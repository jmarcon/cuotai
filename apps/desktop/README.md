# Cuotai Desktop (Tauri 2)

Front-end multiplataforma: janela principal + popover na bandeja. Sem npm/bundler:
a UI é HTML/CSS/JS puro em `ui/` e o shell é Rust em `src-tauri/`. Só lê `cuotai-core`.

```sh
cd apps/desktop/src-tauri
cargo run                 # desenvolvimento
cargo install tauri-cli   # opcional, para empacotar
cargo tauri build
```

Config: `CUOTAI_CONFIG`, `./cuotai.toml` ou `<config do SO>/cuotai/config.toml`.
Temas: `ui/css/themes.css` (+ `DARK_THEMES` em `ui/js/lib.js`); padrão escuro = Dracula.
