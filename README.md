# Arvumi CLI

The command-line client for [Arvumi](https://arvumi.com), a deployment
platform. Trigger deployments, stream build logs, manage environments and custom
domains, configure functions, and connect to hosted PostgreSQL databases.

The Cargo package is **`arvumi-cli`**; the installed executable is **`arvumi`**.
It connects to an existing Arvumi server.

## Install

Requires **Rust 1.95 or newer** and the native linker/toolchain for your platform.
Install Rust with [rustup](https://rustup.rs/). Linux, macOS, and Windows are
covered by the package verification workflow. TLS uses Rustls; OpenSSL is not
required.

After the first crates.io release:

```sh
cargo install arvumi-cli --locked
arvumi --version
arvumi --help
```

Until that release, install from the repository root:

```sh
cargo install --path crates/arvumi-cli --locked
```

Cargo normally installs executables in `~/.cargo/bin` (or `%USERPROFILE%\.cargo\bin`
on Windows); ensure that directory is in your `PATH`. Upgrade by running the
registry installation command again after a new version is released.

## Authenticate

Sign in through your browser:

```sh
arvumi login
```

`--key` is also supported for scripting. `arvumi config` shows the selected server
and whether credentials are configured, without displaying their values.

## Link and deploy

Create a project in the dashboard, or use `github-repos` and `create-project`
with a repository grant configured in your team's settings. Then:

```sh
arvumi link <project-uuid>
arvumi deploy
arvumi list <project-uuid>
arvumi logs <deployment-uuid>
```

`deploy` triggers a build of the project's connected Git repository and streams
its build logs. It does **not** upload local files. Commit and push your changes
before deploying. If available, the CLI uses the current local branch; otherwise 
select a branch explicitly with `arvumi deploy --branch main`.

## Manage projects

```sh
# Environment variables require an explicit scope: production, preview or a UUID.
arvumi env --environment production list
arvumi env --environment preview set EXAMPLE value
arvumi env --environment preview delete EXAMPLE

arvumi environments list
arvumi environments create --name staging --branch develop --subdomain staging-myapp

arvumi domain add www.example.com
arvumi domain check www.example.com --wait
arvumi domain target www.example.com --production

arvumi functions config
arvumi functions config --timeout-seconds 60 --memory-mib 512

arvumi teams list
arvumi github-repos --team <team-uuid>
arvumi create-project my-app --team <team-uuid> --github-repository-grant <grant-uuid>
```

Use `arvumi <command> --help` to see arguments and subcommands. Server permissions,
resource limits, and your project's framework support still apply.

## Database access

```sh
arvumi db connect --environment production
arvumi db sessions --environment production
arvumi db disconnect <session-uuid> --project <project-uuid>
```

Database access opens a loopback listener on `127.0.0.1:15432` and carries traffic
through an authenticated encrypted WebSocket tunnel. It requires HTTPS, with
HTTP allowed only for loopback development. Sessions default to read-only for
30 minutes; `--write` requires team owner/admin permissions. Keep the tunnel
running while using the printed connection command. Install `psql` separately,
or use another PostgreSQL client. Treat the printed connection password as a
secret. Press Ctrl-C to close the tunnel and request session revocation.

## Local files

Credentials are stored as JSON in the platform configuration directory:

| Platform | Default path |
| --- | --- |
| Linux | `$XDG_CONFIG_HOME/arvumi/config.json`, or `~/.config/arvumi/config.json` |
| macOS | `~/Library/Application Support/arvumi/config.json` |
| Windows | `%APPDATA%\arvumi\config.json` |

The file contains credentials in plaintext; keep it private. On Unix the CLI
creates it with mode `0600` and refuses a symlink at that path. Windows uses the
configuration directory's inherited access permissions.

`arvumi link` writes the project UUID to `.arvumi` in the current directory.
Add `.arvumi` to your project's `.gitignore` if links differ between developers (uncommon).

