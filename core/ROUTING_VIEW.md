# Effective routing inspection and balancer recovery

`Native.routing(config, profile, options)` returns a JSON string, or `{"error":"..."}`.
The desktop request is `{"cmd":"routing","text":"...","profile":"...","options":{},"tun":true,"req":...}`;
response `{"ev":"routing","req":...,"routing":{...}}` or `{"ev":"routing","req":...,"error":"..."}`.
Desktop `tun` must match connection mode, so preview includes the same local-name/TUN DNS policy as the live engine.

The result has schema `version:1`, `mode`, `source`, `defaultRoute`, `domainStrategy`,
`rules:[{source,rule}]`, `dns`, `outbounds:[{tag,kind}]`, `balancers`, `ipv4Only`, `warnings`, `smartDirect`.
Rule `source` is `user`, `local`, `smart`, `subscription`, or `global`.
Only `smart` or `subscription` belongs in the single pinned read-only built-in set.
`rule` is one original Xray field rule: its different fields are conjunctive, values within
an array have the original router's semantics. Keep this object intact in the viewer;
do not split its domain/IP/process/network/port/inbound fields into independent rules.
`geoip:`/`geosite:`/external database references remain references, never expanded.

The snapshot comes from the same `Engine::build` compilation used by `Engine::new`.
Live engines do not retain an extra snapshot/copy of their route table. Inspection starts
no sockets, observer, keepalive or timer. Android options may include `geoDir` to load
installed geo data for validation; references are retained even if a database is missing,
and warnings make that condition visible. DNS includes the effective ordered servers,
each original `definition`, normalized destination, family/timeout/fallback flags,
plus original policy and user override. `smartDirect` describes Smart's real direct-first
route and proxy fallback rather than pretending that direct never falls back.

## #9093 recovery

Xray runs observatory/burstObservatory periodically, even without application traffic.
ryNet deliberately avoids idle periodic probes. Previously only startup/network change
and selected-outbound traffic updated measurements, so a healthy CDN masked recovery
of dead ordinary candidates indefinitely (until stale measurements happened to permit a retry).

Now a real routing request that selects a measured balancer's explicit fallback may
start a bounded recovery probe. The configured observatory interval is honored in the
battery-safe range 15–300 seconds (default 60); failed batches back off to 300 seconds.
There is no idle timer. A batch probes at most four candidates, with rotation and a global
four-probe cap; concurrent/recent probes of the same tag are shared across balancers.
Probe results are guarded by network generation, pending tasks are aborted on reset,
and all affected measured choices and UDP route cache are refreshed after a batch.
Existing CDN connections are not interrupted/migrated. Newly routed connections use
recovered ordinary candidates after the probe succeeds. No new routing request means
no recovery probe and no idle wakeup; long-lived working CDN flows remain on CDN.

`RECOVERY_XRAY=/path/to/xray cargo test -p rv-core --test balancer_recovery -- --ignored --nocapture`
uses real Xray SOCKS, a local transport gate, and leastLoad -> loopback -> CDN rules.
It verifies normal -> CDN -> normal, zero probes over 15.1 seconds idle on CDN, and
exactly one probe on the next routing request. All sockets and destinations are loopback.
