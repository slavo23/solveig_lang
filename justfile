install:
    cargo build --release
    cargo install --path .
    cd solveig_vscode && npm install
    cd solveig_vscode && npx vsce package --out /tmp/solveig.vsix
    code --install-extension /tmp/solveig.vsix --force

uninstall:
    code --uninstall-extension solveig-lang.solveig
