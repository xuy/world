#!/bin/sh
set -e

INSTALL_DIR="${INSTALL_DIR:-$HOME/.local/bin}"

echo "Building world (release)..."
cargo build --release

echo "Installing to $INSTALL_DIR/world..."
mkdir -p "$INSTALL_DIR"
cp target/release/world "$INSTALL_DIR/world"

# Shell completions
case "$SHELL" in
  */zsh)
    COMP_DIR="${XDG_DATA_HOME:-$HOME/.local/share}/zsh/site-functions"
    mkdir -p "$COMP_DIR"
    "$INSTALL_DIR/world" completions zsh > "$COMP_DIR/_world"
    echo "Zsh completions installed to $COMP_DIR/_world"
    ;;
  */bash)
    COMP_DIR="${XDG_DATA_HOME:-$HOME/.local/share}/bash-completion/completions"
    mkdir -p "$COMP_DIR"
    "$INSTALL_DIR/world" completions bash > "$COMP_DIR/world"
    echo "Bash completions installed to $COMP_DIR/world"
    ;;
  */fish)
    COMP_DIR="${XDG_CONFIG_HOME:-$HOME/.config}/fish/completions"
    mkdir -p "$COMP_DIR"
    "$INSTALL_DIR/world" completions fish > "$COMP_DIR/world.fish"
    echo "Fish completions installed to $COMP_DIR/world.fish"
    ;;
  *)
    echo "Unknown shell ($SHELL) — skipping completions."
    ;;
esac

echo ""
echo "Installed: $(command -v world || echo "$INSTALL_DIR/world")"
echo "Version:  $("$INSTALL_DIR/world" --version)"

# Check PATH
case ":$PATH:" in
  *":$INSTALL_DIR:"*) ;;
  *) echo ""; echo "Add to PATH: export PATH=\"$INSTALL_DIR:\$PATH\"" ;;
esac
