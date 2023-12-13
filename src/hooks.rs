//! Lifecycle hooks.

use std::fmt::Display;
use std::fmt::Write;
use std::process::ExitStatus;
use std::str::FromStr;

use clap::builder::ValueParserFactory;
use clap::Arg;
use clap::ArgAction;
use clap::Args;
use clap::FromArgMatches;
use enum_iterator::Sequence;
use indoc::indoc;
use tokio::task::JoinHandle;

use crate::ghci::GhciCommand;
use crate::maybe_async_command::MaybeAsyncCommand;

/// A lifecycle event that triggers hooks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Sequence)]
pub enum LifecycleEvent {
    /// When tests are run (after startup, after reloads).
    Test,
    /// When a `ghci` session is started (at `ghciwatch` startup and after restarts).
    Startup(When),
    /// When a module is changed or added.
    Reload(When),
    /// When a `ghci` session is restarted (when a module is removed or renamed).
    Restart(When),
}

impl Display for LifecycleEvent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if let Some(when) = self.when() {
            write!(f, "{}-", when)?;
        }
        write!(f, "{}", self.event_name())
    }
}

impl LifecycleEvent {
    /// Get the event name, like `test` or `reload`.
    pub fn event_name(&self) -> &'static str {
        match self {
            LifecycleEvent::Test => "test",
            LifecycleEvent::Startup(_) => "startup",
            LifecycleEvent::Reload(_) => "reload",
            LifecycleEvent::Restart(_) => "restart",
        }
    }

    /// Get the noun form of the event name, like `testing` or `reloading`.
    pub fn event_noun(&self) -> &'static str {
        match self {
            LifecycleEvent::Test => "testing",
            LifecycleEvent::Startup(_) => "starting up",
            LifecycleEvent::Reload(_) => "reloading",
            LifecycleEvent::Restart(_) => "restarting",
        }
    }

    fn get_message(&self) -> &'static str {
        match self {
            LifecycleEvent::Test => indoc!(
                "
                Tests are run after startup and after reloads.
                "
            ),
            LifecycleEvent::Startup(_) => indoc!(
                "
                Startup hooks run when `ghci` is started (at `ghciwatch` startup and after `ghci` restarts).
                "
            ),
            LifecycleEvent::Reload(_) => indoc!(
                "
                Reload hooks are run when modules are changed on disk.
                "
            ),
            LifecycleEvent::Restart(_) => indoc!(
                "
                `ghci` is restarted when modules are removed or renamed.
                See: https://gitlab.haskell.org/ghc/ghc/-/issues/11596
                "
            ),
        }.trim_end_matches('\n')
    }

    fn get_help_name(&self) -> Option<&'static str> {
        match self {
            LifecycleEvent::Test => Some("tests"),
            _ => None,
        }
    }

    fn when(&self) -> Option<When> {
        match &self {
            LifecycleEvent::Test => None,
            LifecycleEvent::Startup(when) => Some(*when),
            LifecycleEvent::Reload(when) => Some(*when),
            LifecycleEvent::Restart(when) => Some(*when),
        }
    }

    fn supported_kind(&self) -> Vec<CommandKind> {
        match self {
            LifecycleEvent::Startup(When::Before) => vec![CommandKind::Shell],
            LifecycleEvent::Startup(When::After)
            | LifecycleEvent::Test
            | LifecycleEvent::Reload(_)
            | LifecycleEvent::Restart(_) => {
                vec![CommandKind::Ghci, CommandKind::Shell]
            }
        }
    }

    fn hooks() -> impl Iterator<Item = Hook<CommandKind>> {
        enum_iterator::all::<Self>().flat_map(|event| {
            event.supported_kind().into_iter().map(move |kind| Hook {
                event,
                command: kind,
            })
        })
    }
}

/// When to run a hook in relation to a given [`LifecycleEvent`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Sequence)]
pub enum When {
    /// Run the hook before the event.
    Before,
    /// Run the hook after the event.
    After,
}

impl Display for When {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            When::Before => write!(f, "before"),
            When::After => write!(f, "after"),
        }
    }
}

/// The kind of hook; a `ghci` command or a shell command.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Sequence)]
pub enum CommandKind {
    /// A shell command.
    Shell,
    /// A `ghci` command.
    ///
    /// Can either be Haskell code to run (`TestMain.test`) or a `ghci` command (`:set args ...`).
    Ghci,
}

impl Display for CommandKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CommandKind::Shell => write!(f, "shell"),
            CommandKind::Ghci => write!(f, "ghci"),
        }
    }
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
    /// The command to run.
    pub command: C,
}

impl<C> Display for Hook<C> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.event.fmt(f)
    }
}

impl<C> Hook<C> {
    fn with_command<C2>(&self, command: C2) -> Hook<C2> {
        Hook {
            event: self.event,
            command,
        }
    }
}

impl Hook<CommandKind> {
    fn extra_help(&self) -> Option<&'static str> {
        match (self.event, self.command) {
            (LifecycleEvent::Startup(When::Before), _) => Some(indoc!(
                "
                This can be used to regenerate `.cabal` files with `hpack`.
                ",
            )),
            (LifecycleEvent::Startup(When::After), CommandKind::Ghci) => Some(indoc!(
                "
                Use `:set args ...` to set command-line arguments for test hooks.
                ",
            )),
            (LifecycleEvent::Test, CommandKind::Ghci) => Some(indoc!(
                "
                Example: `TestMain.testMain`.
                ",
            )),
            _ => None,
        }
        .map(|help| help.trim_end_matches('\n'))
    }

    fn arg_name(&self) -> String {
        format!("{}-{}", self.event, self.command)
    }

    fn help(&self) -> Help {
        let Hook { event, command } = self;
        let kind = match command {
            CommandKind::Ghci => "`ghci`",
            CommandKind::Shell => "Shell",
        };

        let mut short = format!("{kind} commands to run");

        if let Some(when) = self.event.when() {
            short.push(' ');
            write!(short, "{}", when).expect("Writing to a `String` never fails");
        }

        short.push(' ');
        if let Some(help_name) = event.get_help_name() {
            short.push_str(help_name);
        } else {
            write!(short, "{}", event.event_name()).expect("Writing to a `String` never fails");
        }

        let mut long = short.clone();

        long.push_str("\n\n");
        long.push_str(event.get_message());

        if let Some(extra_help) = self.extra_help() {
            long.push('\n');
            long.push_str(extra_help);
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
    pub fn select(&self, event: LifecycleEvent) -> impl Iterator<Item = &Hook<Command>> {
        self.hooks.iter().filter(move |hook| hook.event == event)
    }

    pub async fn run_shell_hooks(
        &self,
        event: LifecycleEvent,
        handles: &mut Vec<JoinHandle<miette::Result<ExitStatus>>>,
    ) -> miette::Result<()> {
        for hook in self.select(event) {
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
