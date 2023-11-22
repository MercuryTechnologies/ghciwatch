//! Lifecycle hooks.

use std::fmt::Display;
use std::process::ExitStatus;
use std::str::FromStr;

use clap::builder::ValueParserFactory;
use clap::Arg;
use clap::ArgAction;
use clap::Args;
use clap::FromArgMatches;
use strum::EnumMessage;
use strum::EnumProperty;
use strum::IntoEnumIterator;
use tokio::task::JoinHandle;

use crate::ghci::GhciCommand;
use crate::maybe_async_command::MaybeAsyncCommand;

/// A lifecycle event that triggers hooks.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    strum::EnumVariantNames,
    strum::EnumIter,
    strum::EnumMessage,
    strum::EnumProperty,
    strum::Display,
    strum::AsRefStr,
)]
#[strum(serialize_all = "kebab-case")]
pub enum LifecycleEvent {
    /// When tests are run (after startup, after reloads).
    #[strum(
        message = "
        Tests are run after startup and after reloads.
        ",
        props(help_name = "tests")
    )]
    Test,
    /// When a `ghci` session is started (at `ghciwatch` startup and after restarts).
    #[strum(message = "
        Startup hooks run when `ghci` is started (at `ghciwatch` startup and after `ghci` restarts).
    ")]
    Startup,
    /// When a module is changed or added.
    #[strum(message = "
        Reload hooks are run when modules are changed on disk.
    ")]
    Reload,
    /// When a `ghci` session is restarted (when a module is removed or renamed).
    #[strum(message = "
        `ghci` is restarted when modules are removed or renamed.
        See: https://gitlab.haskell.org/ghc/ghc/-/issues/11596
    ")]
    Restart,
}

impl LifecycleEvent {
    fn supported_when(&self) -> Vec<When> {
        match self {
            LifecycleEvent::Test => vec![When::During],
            LifecycleEvent::Startup | LifecycleEvent::Reload | LifecycleEvent::Restart => {
                vec![When::Before, When::After]
            }
        }
    }

    fn supported_kind(&self, when: When) -> Vec<CommandKind> {
        debug_assert!(self.supported_when().contains(&when));
        match self {
            LifecycleEvent::Reload | LifecycleEvent::Restart | LifecycleEvent::Test => {
                vec![CommandKind::Ghci, CommandKind::Shell]
            }
            LifecycleEvent::Startup => match when {
                When::Before => vec![CommandKind::Shell],
                When::During => unreachable!(),
                When::After => vec![CommandKind::Ghci, CommandKind::Shell],
            },
        }
    }

    fn hooks() -> impl Iterator<Item = Hook<CommandKind>> {
        Self::iter()
            .flat_map(|event| {
                event
                    .supported_when()
                    .into_iter()
                    .map(move |when| (event, when))
            })
            .flat_map(|(event, when)| {
                event
                    .supported_kind(when)
                    .into_iter()
                    .map(move |kind| Hook {
                        event,
                        when,
                        command: kind,
                    })
            })
    }
}

/// When to run a hook in relation to a given [`LifecycleEvent`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, strum::Display, strum::AsRefStr)]
#[strum(serialize_all = "kebab-case")]
pub enum When {
    /// Run the hook before the event.
    Before,
    /// Run the hook at the event.
    ///
    /// This is used for the test hook.
    During,
    /// Run the hook after the event.
    After,
}

/// The kind of hook; a `ghci` command or a shell command.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, strum::Display, strum::AsRefStr)]
#[strum(serialize_all = "kebab-case")]
pub enum CommandKind {
    /// A shell command.
    Shell,
    /// A `ghci` command.
    ///
    /// Can either be Haskell code to run (`TestMain.test`) or a `ghci` command (`:set args ...`).
    Ghci,
}

impl CommandKind {
    fn placeholder_name(&self) -> &'static str {
        match self {
            CommandKind::Ghci => "GHCI_CMD",
            CommandKind::Shell => "SHELL_CMD",
        }
    }
}

/// A command to run for a hook.
#[derive(Debug, Clone)]
pub enum Command {
    /// A shell command.
    Shell(MaybeAsyncCommand),
    /// A `ghci` command.
    Ghci(GhciCommand),
}

impl Command {
    fn kind(&self) -> CommandKind {
        match self {
            Command::Ghci(_) => CommandKind::Ghci,
            Command::Shell(_) => CommandKind::Shell,
        }
    }
}

impl Display for Command {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Command::Ghci(command) => command.fmt(f),
            Command::Shell(command) => command.fmt(f),
        }
    }
}

/// A lifecycle hook, specifying a command to run and an event to run it at.
#[derive(Debug, Clone)]
pub struct Hook<C> {
    /// The event to run this hook on.
    pub event: LifecycleEvent,
    /// When to run the hook in relation to the event.
    pub when: When,
    /// The command to run.
    pub command: C,
}

impl<C> Display for Hook<C> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.when {
            When::During => write!(f, "{}", self.event.as_ref()),
            _ => write!(f, "{}-{}", self.when, self.event),
        }
    }
}

impl<C> Hook<C> {
    fn with_command<C2>(&self, command: C2) -> Hook<C2> {
        Hook {
            event: self.event,
            when: self.when,
            command,
        }
    }
}

impl Hook<CommandKind> {
    fn extra_help(&self) -> Option<&'static str> {
        match (self.event, self.when, self.command) {
            (LifecycleEvent::Startup, When::Before, _) => Some(
                "
                This can be used to regenerate `.cabal` files with `hpack`.
                ",
            ),
            (LifecycleEvent::Startup, When::After, CommandKind::Ghci) => Some(
                "
                Use `:set args ...` to set command-line arguments for test hooks.
                ",
            ),
            (LifecycleEvent::Test, _, CommandKind::Ghci) => Some(
                "
                Example: `TestMain.testMain`.
                ",
            ),
            _ => None,
        }
    }

    fn arg_name(&self) -> String {
        let mut ret = match self.when {
            When::During => String::new(),
            when => when.to_string(),
        };
        if !ret.is_empty() {
            ret.push('-');
        }
        ret.push_str(self.event.as_ref());
        ret.push('-');
        ret.push_str(self.command.as_ref());
        ret
    }

    fn help(&self) -> Help {
        let Hook {
            event,
            when,
            command,
        } = self;
        let kind = match command {
            CommandKind::Ghci => "`ghci`",
            CommandKind::Shell => "Shell",
        };

        let mut short = format!("{kind} commands to run");

        match when {
            When::During => {}
            _ => {
                short.push(' ');
                short.push_str(when.as_ref());
            }
        }

        short.push(' ');
        if let Some(help_name) = event.get_str("help_name") {
            short.push_str(help_name);
        } else {
            short.push_str(event.as_ref());
        }

        let mut long = short.clone();

        if let Some(message) = self.event.get_message() {
            long.push_str("\n\n");
            long.push_str(unindent::unindent(message).trim_end_matches('\n'));
        }
        if let Some(extra_help) = self.extra_help() {
            long.push('\n');
            long.push_str(unindent::unindent(extra_help).trim_end_matches('\n'));
        }

        long.push_str("\n\nCan be given multiple times.");

        Help { short, long }
    }
}

struct Help {
    short: String,
    long: String,
}

/// Lifecycle hooks.
///
/// These are `ghci` and shell commands to run at various points in the `ghciwatch`
/// lifecycle.
#[derive(Debug, Clone, Default)]
pub struct HookOpts {
    hooks: Vec<Hook<Command>>,
}

impl HookOpts {
    pub fn select(
        &self,
        event: LifecycleEvent,
        when: When,
    ) -> impl Iterator<Item = &Hook<Command>> {
        self.hooks
            .iter()
            .filter(move |hook| hook.event == event && hook.when == when)
    }

    pub async fn run_shell_hooks(
        &self,
        event: LifecycleEvent,
        when: When,
        handles: &mut Vec<JoinHandle<miette::Result<ExitStatus>>>,
    ) -> miette::Result<()> {
        for hook in self.select(event, when) {
            if let Command::Shell(command) = &hook.command {
                tracing::info!(%command, "Running {hook} command");
                command.run_on(handles).await?;
            }
        }
        Ok(())
    }
}

impl Args for HookOpts {
    fn augment_args(mut cmd: clap::Command) -> clap::Command {
        for hook in LifecycleEvent::hooks() {
            let name = hook.arg_name();
            let help = hook.help();
            let arg = Arg::new(&name)
                .long(&name)
                .action(ArgAction::Append)
                .required(false)
                .value_name(hook.command.placeholder_name())
                .help(help.short)
                .long_help(help.long)
                .help_heading("Lifecycle hooks");

            let arg = match hook.command {
                CommandKind::Ghci => arg.value_parser(GhciCommand::value_parser()),
                CommandKind::Shell => arg.value_parser(MaybeAsyncCommand::from_str),
            };

            cmd = cmd.arg(arg);
        }
        cmd
    }

    fn augment_args_for_update(cmd: clap::Command) -> clap::Command {
        Self::augment_args(cmd)
    }
}

impl FromArgMatches for HookOpts {
    fn from_arg_matches(matches: &clap::ArgMatches) -> Result<Self, clap::Error> {
        let mut ret = Self::default();
        ret.update_from_arg_matches(matches)?;
        Ok(ret)
    }

    fn update_from_arg_matches(&mut self, matches: &clap::ArgMatches) -> Result<(), clap::Error> {
        for hook in LifecycleEvent::hooks() {
            let name = hook.arg_name();
            match hook.command {
                CommandKind::Ghci => {
                    self.hooks.extend(
                        matches
                            .get_many::<GhciCommand>(&name)
                            .into_iter()
                            .flatten()
                            .map(|command| hook.with_command(Command::Ghci(command.clone()))),
                    );
                }
                CommandKind::Shell => {
                    self.hooks.extend(
                        matches
                            .get_many::<MaybeAsyncCommand>(&name)
                            .into_iter()
                            .flatten()
                            .map(|command| hook.with_command(Command::Shell(command.clone()))),
                    );
                }
            }
        }

        // Sort the hooks so that shell commands are first.
        //
        // Shell commands _may_ be asynchronous, but `ghci` commands are always synchronous, so we
        // run shell commands first.
        self.hooks
            .sort_by(|a, b| a.command.kind().cmp(&b.command.kind()));

        Ok(())
    }
}
