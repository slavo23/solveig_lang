# Installing VSCode Extension

```bash
npm install
code --install-extension solveig-0.1.0.vsix
```

# Rebuilding the extension

```bash
npx vsce package
code --install-extension solveig-0.1.0.vsix --force
```