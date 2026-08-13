install:
    cargo build --release
    cargo install --path .
    install_root="${CARGO_INSTALL_ROOT:-${CARGO_HOME:-$HOME/.cargo}}"; library_root="$install_root/bin/lib"; mkdir -p "$library_root/prelude" "$library_root/std"; cp -R lib/prelude/. "$library_root/prelude"; cp -R lib/std/. "$library_root/std"
    cd solveig_vscode && npm install
    cd solveig_vscode && npx vsce package --out /tmp/solveig.vsix
    code --install-extension /tmp/solveig.vsix --force

uninstall:
    code --uninstall-extension solveig-lang.solveig
