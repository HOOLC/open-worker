/* Stream forwarding adapted from Synchronicity v0.1.8 examples (MIT).
 * Upstream copyright and license: ../proto/LICENSE.
 * Fixed Zork ingress bridge. Built with the pinned Synchronicity compiler.
 * ZORK_MESH_PORT is replaced with an already-bound loopback port at deployment.
 * The local bridge token is activation config, never compiled into this object.
 */
#include <synch.h>
#define PORT ZORK_MESH_PORT
#define TIMEOUT 10000
SY_MANIFEST("{\"manifest\":1,\"name\":\"zork-mesh\",\"max_streams\":128,"
            "\"egress\":[\"127.0.0.1:" SY_STRINGIZE(PORT) "\"]}");

SY_ENTRY sy_s64 entry(void) {
    char token[65], origin[257], hex[65];
    sy_u8 key[32];
    if (!sy_peer_has_space(SY_STR("zork-control"))) return 1;
    sy_s64 n = sy_config_get(SY_STR("bridge_token"), token, sizeof token);
    if (n != 64) return 2;
    sy_s64 origin_len = sy_peer_origin(origin, sizeof origin);
    if (origin_len <= 0 || origin_len >= sizeof origin) return 2;
    if (sy_peer_device_key(key) < 0) return 2;
    sy_hex_encode(key, sizeof key, hex, sizeof hex, 0);
    sy_s64 up = sy_tcp_connect(SY_STR("127.0.0.1"), PORT);
    if (up < 0) return 3;
    /* No caller bytes are read before this authenticated prelude is written. */
    if (sy_write_all(up, SY_STR("ZORKMESH1\n"), TIMEOUT) < 0 ||
        sy_write_all(up, token, 64, TIMEOUT) < 0 ||
        sy_write_all(up, SY_STR("\n"), TIMEOUT) < 0 ||
        sy_write_all(up, origin, origin_len, TIMEOUT) < 0 ||
        sy_write_all(up, SY_STR("\n"), TIMEOUT) < 0 ||
        sy_write_all(up, hex, 64, TIMEOUT) < 0 ||
        sy_write_all(up, SY_STR("\n"), TIMEOUT) < 0) {
        sy_close(up);
        return 4;
    }
    int caller_done = 0, upstream_done = 0;
    int upward_blocked = 0, downward_blocked = 0;
    while (!(caller_done && upstream_done)) {
        struct sy_pollfd fds[2] = {{SY_SELF, 0, 0}, {up, 0, 0}};
        if (!caller_done) {
            if (upward_blocked) fds[1].events |= SY_POLL_OUT;
            else fds[0].events |= SY_POLL_IN;
        }
        if (!upstream_done) {
            if (downward_blocked) fds[0].events |= SY_POLL_OUT;
            else fds[1].events |= SY_POLL_IN;
        }
        sy_u64 count = 2;
        if (fds[0].events == 0) { fds[0] = fds[1]; count = 1; }
        else if (fds[1].events == 0) count = 1;
        if (sy_poll(fds, count, -1) <= 0) break;
        if (!caller_done) {
            sy_s64 moved = sy_splice(SY_SELF, up, 32768);
            if (moved == 0) { sy_shutdown(up); caller_done = 1; }
            else if (moved < 0 && moved != SY_EAGAIN) break;
            else upward_blocked = sy_readable(SY_SELF) > 0 && sy_writable(up) == 0;
        }
        if (!upstream_done) {
            sy_s64 moved = sy_splice(up, SY_SELF, 32768);
            if (moved == 0) { sy_shutdown(SY_SELF); upstream_done = 1; caller_done = 1; }
            else if (moved < 0 && moved != SY_EAGAIN) break;
            else downward_blocked = sy_readable(up) > 0 && sy_writable(SY_SELF) == 0;
        }
        sy_u32 revents = 0;
        for (sy_u64 i = 0; i < count; i++) revents |= fds[i].revents;
        if (revents & SY_POLL_ERR) break;
    }
    sy_close(up);
    sy_shutdown(SY_SELF);
    return caller_done && upstream_done ? 0 : 5;
}
