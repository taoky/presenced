# presenced

- presenced: Opens `$XDG_RUNTIME_DIR/app/com.discordapp.Discord/discord-ipc-0` and `$XDG_RUNTIME_DIR/run/user/1000/discord-ipc-0`, acts like a Discord client, and sends state info to given HTTP server.
- presence-http: A simple HTTP server that listens for `presenced` POSTed data at `/state`, and gives a friendly HTML at `/`.

## Deployment

### presenced

1. Copy compiled `presenced` to `~/.local/bin/`, and make it executable.
2. Copy `presenced.env.example` to `~/.config/presenced.env`, and fill in the values.
3. Copy `presenced.service` to `~/.config/systemd/user/presenced.service`, `systemctl --user daemon-reload`, and `systemctl --user enable --now presenced.service`.

### presence-http

Use `docker compose`. Please refer to `contrib/presence_http/`.

## License

MIT.
