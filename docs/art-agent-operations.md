# Create an art-agent connection

Audience: server operators.

An operator runs this on the Infuser host, as the service user:

```sh
sudo -u infuser infuser metadata-pass /etc/infuser/infuser.toml \
  /var/lib/infuser/art-agent.credentials.json --all-groups
```

The command creates a mode-0600 file with `api_url`, `token`, `id`, and
`scope:"all_games_metadata"`. It refuses to overwrite a file and never prints the
secret. Transfer it privately to the art agent; keep it out of Git, logs, game
exports, and player prompts. An operator who is also a home-group admin can revoke it under Admin tools → Agent access
(the row is labeled **Infuser art agent (all groups)**). Revocation takes effect on
the next request. It is an operator service credential, independent of a user's
membership; only an operator can create one.

For a single private group, replace `--all-groups` with `--group GROUP_ID`.
Without either flag, access is limited to `home`. Group keys have
`scope:"group_metadata"` and `group_id`; their lists and updates are restricted
to that group, and its admins can revoke them. Previously issued all-games
keys keep their installation-wide access. See [private groups](private-groups.md).

Give the art agent its credential file and the [art-agent guide](art-agent.md).
