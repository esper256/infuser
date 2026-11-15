# Network admission

Published native game ports are closed by default. After an invited player logs
in, the platform grants their observed source IP temporary access, renewed
while their authenticated session is active. This is an extra scanner barrier,
not player identity: everyone behind the same public IP shares access. Games
still need short-lived launch tickets and their own player authorization.

The platform grants and refreshes these addresses through the optional Unix-socket
firewall helper after Google session and Infuser membership checks pass. See
[Infuser installation](clubhouse-operations.md) for setup. Web games use HTTPS
and scoped host-only game sessions.

The nftables table in `deploy/game-ingress.nft` filters incoming TCP and UDP
traffic to local game ports before Docker's DNAT. Filtering only host INPUT or
UFW rules is insufficient for Docker-published ports. The table covers IPv4 and
IPv6, does not change Docker's own tables, and does not touch SSH or HTTPS.
Its original-port match avoids depending on each game's internal port.

Install the rules under `/etc/infuser/game-ingress.nft`, the loader as
`/usr/local/libexec/infuser-firewall-load`, and the service under
`/etc/systemd/system/infuser-firewall.service`. Enable it before exposing games.
Keep the configured port range aligned with `controller.port_start/port_end`.
Install `deploy/infuser-firewall.conf` as
`/etc/systemd/system/infuser.service.d/firewall.conf` on installations using this
policy. It makes firewall startup a prerequisite for controller startup.

For an operator test, a short grant can be added with:

```sh
sudo nft add element inet infuser_ingress members4 '{ 192.0.2.123 timeout 60s }'
```

The kernel expires grants even if the platform crashes. Restarting the firewall
atomically clears grants; it never flushes unrelated firewall tables. The
login integration must obtain the address from the actual connection or the
explicitly trusted reverse proxy, never an arbitrary client-supplied header or
request field. Multiple sessions sharing an IP need combined expiry tracking;
one logout must not revoke another active player's access.

References: [Docker packet filtering](https://docs.docker.com/engine/network/firewall-iptables/)
and [Docker with nftables](https://docs.docker.com/engine/network/firewall-nftables/).
