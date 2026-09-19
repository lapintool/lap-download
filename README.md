# lapdw

Windows 桌面多线程下载器（Tauri 2 + Vue 3 + lapstyle）。

## 开发

```bash
pnpm install
pnpm dev
```

开发服务默认从 `3415` 起找空闲端口；若被占用会自动顺延，启动日志里会打印实际端口。

## 打包（Windows NSIS）

```bash
pnpm build
```

## 便携数据

- `config/settings.json` — HF token、代理、线程、下载目录
- `config/window.json` — 窗口位置
- `data/tasks.json` — 任务队列
- `downloads/` — 默认下载目录
