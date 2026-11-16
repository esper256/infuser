// This is the shared web server build, not part of the Rust controller or SDK.
package main

import (
	_ "github.com/caddy-dns/porkbun"
	caddycmd "github.com/caddyserver/caddy/v2/cmd"
	_ "github.com/caddyserver/caddy/v2/modules/standard"
)

func main() { caddycmd.Main() }
