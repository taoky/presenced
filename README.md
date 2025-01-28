# presenced

- presenced: Opens `$XDG_RUNTIME_DIR/app/com.discordapp.Discord/discord-ipc-0` and `$XDG_RUNTIME_DIR/run/user/1000/discord-ipc-0`, acts like a Discord client, and sends state info to given HTTP server.
- presence_http: A simple HTTP server that listens for `presenced` POSTed data at `/state`, and gives a friendly HTML at `/`.
- os_presence: An example client that reports kernel version, distro & desktop environment name, and uptime to `presenced`.

## Deployment

### presenced

1. Copy compiled `presenced` to `~/.local/bin/`, and make it executable.
2. Copy `presenced.env.example` to `~/.config/presenced.env`, and fill in the values.
3. Copy `presenced.service` to `~/.config/systemd/user/presenced.service`, `systemctl --user daemon-reload`, and `systemctl --user enable --now presenced.service`.

```shell
cargo build --release
install -Dm755 target/release/presenced ~/.local/bin/presenced
install -Dm644 contrib/presenced/presenced.env.example ~/.config/presenced.env
# Edit ~/.config/presenced.env
install -Dm644 contrib/presenced/presenced.service ~/.config/systemd/user/presenced.service
systemctl --user daemon-reload
systemctl --user enable --now presenced.service
```

### presence_http

Use `docker compose`. Please refer to `contrib/presence_http/`.

### os_presence

```shell
cargo build --release
install -Dm755 target/release/os_presence ~/.local/bin/os_presence
install -Dm644 contrib/os_presence/os_presence.service ~/.config/systemd/user/os_presence.service
systemctl --user daemon-reload
systemctl --user enable --now os_presence.service
```

## License

MIT.
