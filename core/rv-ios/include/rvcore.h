#pragma once
#include <stdint.h>

typedef void (*rv_exit_cb)(void *ctx, const char *why);
typedef void (*rv_ping_cb)(void *ctx, const char *name, int32_t ms, const char *err);
typedef void (*rv_progress_cb)(void *ctx, int32_t phase, double value, double fraction);
typedef void (*rv_server_cb)(void *ctx, const char *json);

void rv_free(char *p);

int32_t rv_tun_fd(void);

void *rv_start(int32_t fd, const char *config, const char *profile, const char *hosts, int32_t mtu,
               const char *geo_dir, const char *options, rv_exit_cb on_exit, void *ctx, char **err);
void rv_stop(void *handle);
void rv_network_changed(void *handle, const char *dns_json);
int32_t rv_reconfigure(void *handle, const char *config, const char *profile, const char *hosts,
                       const char *geo_dir, const char *options, char **err);
void *rv_resolver(void *handle);
char *rv_resolver_resolve(void *resolver, const char *name);
void rv_resolver_free(void *resolver);
void rv_add_hosts(const char *hosts_json);
void rv_flush_log(void);

char *rv_describe(const char *text, char **err);
char *rv_ping(const char *config, const char *remarks_json, int32_t mode, int32_t parallel, int32_t timeout_ms,
              const char *ping_url, const char *tune, rv_ping_cb cb, void *ctx, char **err);
char *rv_speedtest(const char *config, const char *remarks, const char *tune, rv_progress_cb progress,
                   rv_server_cb server, void *ctx, char **err);
void rv_speedtest_cancel(void);
char *rv_bootstrap_hosts(const char *names_json, const char *mode, char **err);
char *rv_share_link(const char *config, const char *remarks);
