#!/bin/sh
#
# Minimal fake GHCi REPL for PTY integration tests.
# Responds to the ghciwatch startup protocol without requiring a real Haskell toolchain.

PROMPT="ghci> "
CWD="$(pwd)"

# Initial version banner (ghciwatch reads until this pattern)
echo "GHCi, version 9.8.4: https://www.haskell.org/ghc/  :? for help"

# After the version banner, ghciwatch sends commands via stdin.
# Respond with appropriate output + current prompt.
while IFS= read -r cmd; do
    case "$cmd" in
        *"set prompt"*)
            # Extract the new prompt text from `:set prompt <text>`
            PROMPT=$(echo "$cmd" | sed 's/:set prompt\(-cont\)\{0,1\} //')
            # Emit compilation progress lines (these go through ProgressWriter)
            echo "[1 of 3] Compiling MyLib ( src/MyLib.hs, interpreted )"
            echo "[2 of 3] Compiling MyModule ( src/MyModule.hs, interpreted )"
            echo "[3 of 3] Compiling TestMain ( test/TestMain.hs, interpreted )"
            echo "Ok, 3 modules loaded."
            printf "%s" "$PROMPT"
            ;;
        *"show paths"*)
            echo "current working directory:"
            echo "  $CWD"
            echo "module import search paths:"
            echo "  src"
            printf "%s" "$PROMPT"
            ;;
        *"show targets"*)
            echo "MyLib"
            echo "MyModule"
            printf "%s" "$PROMPT"
            ;;
        *"quit"*)
            echo "Leaving GHCi."
            exit 0
            ;;
        *)
            printf "%s" "$PROMPT"
            ;;
    esac
done
