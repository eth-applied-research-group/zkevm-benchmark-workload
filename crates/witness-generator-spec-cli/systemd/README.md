# witness-generator-spec-cli systemd units

This directory contains example systemd units for running a stateless input
collector, periodically exporting complete local batches, and periodically
publishing those batches plus the generated public catalog to Cloudflare R2.

The units assume:

- Binary path: `/usr/local/bin/witness-generator-spec-cli`
- Runtime user and group: `stateless-inputs`
- Config path: `/etc/witness-generator-spec-cli/glamsterdam-devnet-8.toml`
- Data root in the CLI config: `/data/stateless-inputs`
- Optional R2 credentials file: `/etc/witness-generator-spec-cli/r2.env`
- `flock` available at `/usr/bin/flock` to serialize export and publish jobs

Adjust the unit files if your network name, config path, binary path, or data
root differs.

## Install

Build and install the CLI:

```bash
cargo build --release -p witness-generator-spec-cli
sudo install -m 0755 target/release/witness-generator-spec-cli \
  /usr/local/bin/witness-generator-spec-cli
```

Create the service user and directories:

```bash
sudo useradd --system --home /data/stateless-inputs \
  --shell /usr/sbin/nologin stateless-inputs
sudo install -d -o stateless-inputs -g stateless-inputs -m 0750 \
  /data/stateless-inputs
sudo install -d -o root -g stateless-inputs -m 0750 \
  /etc/witness-generator-spec-cli
```

Create the CLI config from the example and edit the RPC URLs, R2 bucket, and
Cloudflare account ID for your deployment:

```bash
sudo install -m 0640 -o root -g stateless-inputs \
  crates/witness-generator-spec-cli/systemd/glamsterdam-devnet-8.toml.example \
  /etc/witness-generator-spec-cli/glamsterdam-devnet-8.toml
sudoedit /etc/witness-generator-spec-cli/glamsterdam-devnet-8.toml
```

Restrict the config file because RPC URLs can be private:

```bash
sudo chown root:stateless-inputs \
  /etc/witness-generator-spec-cli/glamsterdam-devnet-8.toml
sudo chmod 0640 /etc/witness-generator-spec-cli/glamsterdam-devnet-8.toml
```

Install the systemd units:

```bash
sudo install -m 0644 crates/witness-generator-spec-cli/systemd/*.service \
  crates/witness-generator-spec-cli/systemd/*.timer /etc/systemd/system/
sudo systemctl daemon-reload
```

Start the live collector and the periodic exporter:

```bash
sudo systemctl enable --now witness-collector.service
sudo systemctl enable --now witness-exporter.timer
```

## R2 publishing

Install and configure the AWS CLI on the host. The `publish-r2` command shells
out to `aws s3 sync`, `aws s3 cp`, and `aws s3 rm` using the Cloudflare R2
endpoint derived from the CLI TOML config.

Create the R2 environment file from the example:

```bash
sudo install -m 0640 -o root -g stateless-inputs \
  crates/witness-generator-spec-cli/systemd/r2.env.example \
  /etc/witness-generator-spec-cli/r2.env
sudoedit /etc/witness-generator-spec-cli/r2.env
```

Then start the publisher timer:

```bash
sudo systemctl enable --now witness-publisher.timer
```

## Operations

Run one export or publish immediately:

```bash
sudo systemctl start witness-exporter.service
sudo systemctl start witness-publisher.service
```

Inspect services and timers:

```bash
systemctl status witness-collector.service \
  witness-exporter.service witness-exporter.timer \
  witness-publisher.service witness-publisher.timer
journalctl -u witness-collector.service -f
systemctl list-timers 'witness-*'
```

If you change the CLI config `out_root`, update `ReadWritePaths` in the unit
files to match the writable data directory.
