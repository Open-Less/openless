// Optional GTK4/WebKitGTK shell. All application commands stay in linux/web.
#include <gtk/gtk.h>
#include <webkit/webkit.h>
#ifdef OPENLESS_GTK_LAYER_SHELL
#include <gtk4-layer-shell.h>
#endif
#include <libsoup/soup.h>
#include <jsc/jsc.h>
#include <algorithm>
#include <cmath>
#include <csignal>
#include <cstdio>
#include <cstring>
#include <string>
#include <sys/stat.h>
#include <sys/file.h>
#include <sys/prctl.h>
#include <sys/socket.h>
#include <sys/un.h>
#include <fcntl.h>
#include <unistd.h>
#include "../../shared/web_host_bridge.h"

namespace {
enum Kind { Main, Capsule, Qa, VoiceEdit, Count };
struct Host;
struct Surface { Host *host = nullptr; Kind kind = Main; GtkWindow *window = nullptr;
    WebKitWebView *view = nullptr; std::string regions = "[]"; };
struct Host {
    GtkApplication *app = nullptr;
    Surface surfaces[Count];
    GSubprocess *backend = nullptr;
    GDataInputStream *output = nullptr;
    GCancellable *cancel = g_cancellable_new();
    SoupSession *http = soup_session_new();
    GInputStream *events = nullptr;
    GSocketService *instance = nullptr;
    std::string base, pending, executable;
    bool minimized = false, stopping = false, windowsReady = false, showRequested = false;
    bool overlaysAvailable = false;
    guint startupTimer = 0, hideTimer = 0;
};
std::string env(const char *name, const char *fallback = "") {
    const char *value = g_getenv(name); return value ? value : fallback;
}
std::string field(JSCValue *value, const char *key) {
    if (!value || !jsc_value_is_object(value)) return {};
    auto *child = jsc_value_object_get_property(value, key);
    char *text = jsc_value_is_string(child) ? jsc_value_to_string(child) : nullptr;
    std::string result = text ? text : ""; g_free(text); g_object_unref(child); return result;
}
double number(JSCValue *value, const char *key) {
    auto *child = jsc_value_object_get_property(value, key);
    double result = jsc_value_is_number(child) ? jsc_value_to_double(child) : NAN;
    g_object_unref(child); return result;
}
void show(Surface &surface, bool visible, bool focus = false) {
    if (surface.kind != Main && !surface.host->overlaysAvailable) return;
    if (visible && focus) gtk_window_present(surface.window);
    else gtk_widget_set_visible(GTK_WIDGET(surface.window), visible);
}
void openUrl(const std::string &url) {
    GUri *uri = g_uri_parse(url.c_str(), G_URI_FLAGS_NONE, nullptr);
    if (!uri) return;
    const char *scheme = g_uri_get_scheme(uri);
    if (g_strcmp0(scheme, "https") == 0 || g_strcmp0(scheme, "http") == 0)
        g_app_info_launch_default_for_uri_async(url.c_str(), nullptr, nullptr, nullptr, nullptr);
    g_uri_unref(uri);
}
void inputRegion(Surface *surface, const std::string &wire) {
    if (surface->kind != Capsule) return;
    surface->regions = wire;
    auto *context = jsc_context_new();
    auto *array = jsc_value_new_from_json(context, wire.c_str());
    if (!array || !jsc_value_is_array(array)) {
        if (array) g_object_unref(array);
        g_object_unref(context); return;
    }
    cairo_region_t *region = cairo_region_create();
    const double width = gtk_widget_get_width(GTK_WIDGET(surface->view));
    const double height = gtk_widget_get_height(GTK_WIDGET(surface->view));
    const double count = number(array, "length");
    for (unsigned i = 0; i < std::min(128.0, count); ++i) {
        auto *r = jsc_value_object_get_property_at_index(array, i);
        if (jsc_value_is_object(r)) {
            double x = number(r, "left"), y = number(r, "top");
            double w = number(r, "width"), h = number(r, "height");
            if (std::isfinite(x + y + w + h) && w > 0 && h > 0) {
                const int left = std::floor(std::clamp(x, 0.0, width));
                const int top = std::floor(std::clamp(y, 0.0, height));
                const int right = std::ceil(std::clamp(x + w, 0.0, width));
                const int bottom = std::ceil(std::clamp(y + h, 0.0, height));
                if (right > left && bottom > top) {
                    cairo_rectangle_int_t rect{left, top, right - left, bottom - top};
                    cairo_region_union_rectangle(region, &rect);
                }
            }
        }
        g_object_unref(r);
    }
    if (g_getenv("OPENLESS_INPUT_REGION_TRACE"))
        g_message("openless inputRects=%d", cairo_region_num_rectangles(region));
    // Empty is genuinely click-through; NULL would restore full-window input.
    if (auto *native = gtk_widget_get_native(GTK_WIDGET(surface->window)))
        if (auto *gdk = gtk_native_get_surface(native)) gdk_surface_set_input_region(gdk, region);
    cairo_region_destroy(region); g_object_unref(array); g_object_unref(context);
}
bool local(Surface *surface, const char *url) {
    const auto &base = surface->host->base;
    return url && g_str_has_prefix(url, (base + "/").c_str());
}
gboolean scriptDialog(WebKitWebView *view, WebKitScriptDialog *dialog, gpointer data) {
    auto *surface = static_cast<Surface *>(data);
    if (!local(surface, webkit_web_view_get_uri(view)) ||
        webkit_script_dialog_get_dialog_type(dialog) != WEBKIT_SCRIPT_DIALOG_ALERT) return FALSE;
    std::string message = webkit_script_dialog_get_message(dialog);
    const std::string copy = "openless-copy:", open = "openless-open:", regions = "openless-input-regions:";
    if (message.rfind(copy, 0) == 0)
        gdk_clipboard_set_text(gtk_widget_get_clipboard(GTK_WIDGET(view)), message.substr(copy.size()).c_str());
    else if (message.rfind(open, 0) == 0) openUrl(message.substr(open.size()));
    else if (message.rfind(regions, 0) == 0) inputRegion(surface, message.substr(regions.size()));
    else return FALSE;
    return TRUE;
}
gboolean navigation(WebKitWebView *, WebKitPolicyDecision *decision, WebKitPolicyDecisionType type, gpointer data) {
    if (type != WEBKIT_POLICY_DECISION_TYPE_NAVIGATION_ACTION &&
        type != WEBKIT_POLICY_DECISION_TYPE_NEW_WINDOW_ACTION) return FALSE;
    auto *surface = static_cast<Surface *>(data);
    auto *action = webkit_navigation_policy_decision_get_navigation_action(WEBKIT_NAVIGATION_POLICY_DECISION(decision));
    const char *url = webkit_uri_request_get_uri(webkit_navigation_action_get_request(action));
    if (local(surface, url) || g_strcmp0(url, "about:blank") == 0) return FALSE;
    if (webkit_navigation_action_is_user_gesture(action)) openUrl(url ? url : "");
    webkit_policy_decision_ignore(decision); return TRUE;
}
gboolean closeWindow(GtkWindow *window, gpointer) { gtk_widget_set_visible(GTK_WIDGET(window), FALSE); return TRUE; }
void createSurface(Host *host, Kind kind) {
    auto &s = host->surfaces[kind]; s.host = host; s.kind = kind;
    s.window = GTK_WINDOW(gtk_application_window_new(host->app));
    gtk_window_set_title(s.window, "OpenLess");
    gtk_window_set_icon_name(s.window, "openless");
    gtk_window_set_default_size(s.window, kind == Main ? 1180 : kind == VoiceEdit ? 680 : kind == Qa ? 480 : 460,
                              kind == Main ? 820 : kind == VoiceEdit ? 520 : kind == Qa ? 80 : 228);
    gtk_window_set_decorated(s.window, kind == Main);
    gtk_window_set_resizable(s.window, kind == Main);
    g_signal_connect(s.window, "close-request", G_CALLBACK(closeWindow), nullptr);
#ifdef OPENLESS_GTK_LAYER_SHELL
    if (kind != Main && host->overlaysAvailable) {
        gtk_layer_init_for_window(s.window);
        gtk_layer_set_namespace(s.window, "openless");
        gtk_layer_set_layer(s.window, GTK_LAYER_SHELL_LAYER_OVERLAY);
        gtk_layer_set_anchor(s.window, GTK_LAYER_SHELL_EDGE_BOTTOM, TRUE);
        gtk_layer_set_margin(s.window, GTK_LAYER_SHELL_EDGE_BOTTOM, 12);
        gtk_layer_set_exclusive_zone(s.window, -1);
        gtk_layer_set_keyboard_mode(s.window, kind == Qa ? GTK_LAYER_SHELL_KEYBOARD_MODE_ON_DEMAND : GTK_LAYER_SHELL_KEYBOARD_MODE_NONE);
    }
#endif
    auto *manager = webkit_user_content_manager_new();
    std::string script = "window.__openlessHost='gtk';window.__openlessEngine='webkitgtk';";
    script += kOpenlessWebHostScript;
    auto *userScript = webkit_user_script_new(script.c_str(), WEBKIT_USER_CONTENT_INJECT_TOP_FRAME,
                                              WEBKIT_USER_SCRIPT_INJECT_AT_DOCUMENT_START, nullptr, nullptr);
    webkit_user_content_manager_add_script(manager, userScript);
    webkit_user_script_unref(userScript);
    s.view = WEBKIT_WEB_VIEW(g_object_new(WEBKIT_TYPE_WEB_VIEW, "user-content-manager", manager, nullptr));
    g_object_unref(manager);
    if (kind != Main) {
        const GdkRGBA transparent{0, 0, 0, 0};
        webkit_web_view_set_background_color(s.view, &transparent);
        gtk_widget_add_css_class(GTK_WIDGET(s.window), "openless-overlay");
    }
    g_signal_connect(s.view, "script-dialog", G_CALLBACK(scriptDialog), &s);
    g_signal_connect(s.view, "decide-policy", G_CALLBACK(navigation), &s);
    g_signal_connect(s.window, "map", G_CALLBACK(+[](GtkWidget *, gpointer data) {
        auto *surface = static_cast<Surface *>(data); inputRegion(surface, surface->regions);
    }), &s);
    gtk_window_set_child(s.window, GTK_WIDGET(s.view));
    const char *route = kind == Capsule ? "capsule" : kind == Qa ? "qa" : "voice-edit";
    std::string url = host->base + "/index.html?bridge=http";
    if (kind != Main) url += std::string("&window=") + route;
    webkit_web_view_load_uri(s.view, url.c_str());
}
void hostEvent(Host *host, const std::string &wire) {
    auto *context = jsc_context_new();
    auto *event = jsc_value_new_from_json(context, wire.c_str());
    if (!event) { g_object_unref(context); return; }
    const auto name = field(event, "name"), payload = field(event, "payload");
    auto *inner = jsc_value_new_from_json(context, payload.c_str());
    auto &main = host->surfaces[Main]; auto &capsule = host->surfaces[Capsule];
    auto &qa = host->surfaces[Qa]; auto &voice = host->surfaces[VoiceEdit];
    if (name == "dictation:state") {
        const auto phase = field(inner, "phase");
        show(capsule, phase == "starting" || phase == "recording" || phase == "transcribing" || phase == "polishing" || phase == "inserting");
    } else if (name == "tray:command") {
        if (payload == "\"quit\"") g_application_quit(G_APPLICATION(host->app));
        else if (payload == "\"show\"") show(main, true, true);
    } else if (name == "voice-edit:show") show(voice, true);
    else if (name == "voice-edit:hide") show(voice, false);
    else if (name == "selection-polish-preview:shown") show(qa, true);
    else if (name == "host:action") {
        const auto action = field(inner, "action");
        if (action == "showMain") show(main, true, true);
        else if (action == "showQa") show(qa, true);
        else if (action == "hideQa") show(qa, false);
        else if (action == "openUrl") openUrl(field(inner, "url"));
        else if (action == "restart") {
            // Release the shared lock and stop our backend before exec; otherwise
            // the replacement would only raise this dying instance and exit.
            g_application_quit(G_APPLICATION(host->app));
            g_object_set_data(G_OBJECT(host->app), "openless-restart", GINT_TO_POINTER(1));
        }
    } else if (name == "selection:state") {
        const auto phase = field(inner, "state");
        if (phase == "polishing" || phase == "transcribing") {
            if (host->hideTimer) { g_source_remove(host->hideTimer); host->hideTimer = 0; }
            show(capsule, true);
        } else {
            if (host->hideTimer) g_source_remove(host->hideTimer);
            host->hideTimer = g_timeout_add(2500, +[](gpointer data) -> gboolean {
                auto *h = static_cast<Host *>(data); h->hideTimer = 0; show(h->surfaces[Capsule], false); return G_SOURCE_REMOVE;
            }, host);
        }
    } else if (name == "qa:state") {
        const auto kind = field(inner, "kind");
        show(qa, !kind.empty() && kind != "idle");
        bool expanded = kind == "thinking" || kind == "answer" || kind == "answerDelta" || kind == "awaitingApproval";
        gtk_window_set_default_size(qa.window, expanded ? 760 : 480, expanded ? 416 : 80);
    }
    if (inner) g_object_unref(inner);
    g_object_unref(event); g_object_unref(context);
}
void connectEvents(Host *host);
void reconnect(Host *host) {
    if (host->events) { g_object_unref(host->events); host->events = nullptr; }
    host->pending.clear();
    if (!host->stopping) g_timeout_add(1000, +[](gpointer data) -> gboolean {
        auto *h = static_cast<Host *>(data); if (!h->stopping) connectEvents(h); return G_SOURCE_REMOVE;
    }, host);
}
void readEvents(Host *host) {
    g_input_stream_read_bytes_async(host->events, 16384, G_PRIORITY_DEFAULT, host->cancel,
        +[](GObject *source, GAsyncResult *result, gpointer data) {
            auto *h = static_cast<Host *>(data); GError *error = nullptr;
            GBytes *bytes = g_input_stream_read_bytes_finish(G_INPUT_STREAM(source), result, &error);
            gsize size = bytes ? g_bytes_get_size(bytes) : 0;
            if (h->stopping || !size) {
                if (bytes) g_bytes_unref(bytes);
                g_clear_error(&error);
                if (!h->stopping) reconnect(h);
                return;
            }
            const auto *text = static_cast<const char *>(g_bytes_get_data(bytes, nullptr));
            h->pending.append(text, size); g_bytes_unref(bytes);
            if (h->pending.size() > 8 * 1024 * 1024) { reconnect(h); return; }
            size_t end;
            while ((end = h->pending.find("\n\n")) != std::string::npos) {
                auto frame = h->pending.substr(0, end); h->pending.erase(0, end + 2);
                if (frame.rfind("data: ", 0) == 0) hostEvent(h, frame.substr(6));
            }
            readEvents(h);
        }, host);
}
void connectEvents(Host *host) {
    SoupMessage *message = soup_message_new("GET", (host->base + "/api/events").c_str());
    soup_session_send_async(host->http, message, G_PRIORITY_DEFAULT, host->cancel,
        +[](GObject *source, GAsyncResult *result, gpointer data) {
            auto *h = static_cast<Host *>(data); GError *error = nullptr;
            auto *stream = soup_session_send_finish(SOUP_SESSION(source), result, &error);
            g_clear_error(&error);
            if (h->stopping) { if (stream) g_object_unref(stream); return; }
            if (!stream) { reconnect(h); return; }
            h->events = stream; readEvents(h);
        }, host);
    g_object_unref(message);
}
void ready(Host *host) {
    if (host->windowsReady) return;
    host->windowsReady = true;
    if (host->startupTimer) { g_source_remove(host->startupTimer); host->startupTimer = 0; }
    for (int i = 0; i < Count; ++i) createSurface(host, static_cast<Kind>(i));
    if (!host->minimized || host->showRequested) show(host->surfaces[Main], true, true);
    connectEvents(host);
}
void readBackend(Host *host) {
    g_data_input_stream_read_line_async(host->output, G_PRIORITY_DEFAULT, host->cancel,
        +[](GObject *source, GAsyncResult *result, gpointer data) {
            auto *h = static_cast<Host *>(data); GError *error = nullptr;
            char *line = g_data_input_stream_read_line_finish(G_DATA_INPUT_STREAM(source), result, nullptr, &error);
            g_clear_error(&error);
            if (!line || h->stopping) { g_free(line); if (!h->stopping) g_application_quit(G_APPLICATION(h->app)); return; }
            if (g_str_has_prefix(line, "OPENLESS_READY=")) {
                char *end = nullptr; const long port = strtol(line + 15, &end, 10);
                if (port > 0 && port < 65536 && end && (*end == 0 || *end == ' ')) {
                    h->base = "http://127.0.0.1:" + std::to_string(port); ready(h);
                }
            } else g_printerr("[backend] %s\n", line);
            g_free(line); readBackend(h);
        }, host);
}
void activate(GtkApplication *app, gpointer data) {
    auto *host = static_cast<Host *>(data); host->app = app;
#ifdef OPENLESS_GTK_LAYER_SHELL
    host->overlaysAvailable = gtk_layer_is_supported();
#endif
    if (!host->overlaysAvailable)
        g_warning("GTK overlay surfaces require gtk4-layer-shell and a layer-shell compositor; use the default Qt frontend for overlays on this desktop");
    auto *css = gtk_css_provider_new();
    gtk_css_provider_load_from_string(css, ".openless-overlay { background: transparent; box-shadow: none; }");
    gtk_style_context_add_provider_for_display(gdk_display_get_default(), GTK_STYLE_PROVIDER(css), GTK_STYLE_PROVIDER_PRIORITY_APPLICATION);
    g_object_unref(css);
    host->base = env("OPENLESS_HOST_URL");
    if (!host->base.empty()) { ready(host); return; }
    auto *launcher = g_subprocess_launcher_new(GSubprocessFlags(G_SUBPROCESS_FLAGS_STDOUT_PIPE | G_SUBPROCESS_FLAGS_STDERR_MERGE));
    g_subprocess_launcher_setenv(launcher, "OPENLESS_HEADLESS", "1", TRUE);
    g_subprocess_launcher_setenv(launcher, "OPENLESS_GUI_EXECUTABLE", env("OPENLESS_LAUNCHER", host->executable.c_str()).c_str(), TRUE);
    g_subprocess_launcher_set_child_setup(launcher, +[](gpointer) {
        const pid_t parent = getppid(); prctl(PR_SET_PDEATHSIG, SIGTERM); if (getppid() != parent) _exit(1);
    }, nullptr, nullptr);
    auto dir = host->executable.substr(0, host->executable.find_last_of('/'));
    auto backend = env("OPENLESS_HOST_BIN", (dir + "/openless-linux-web").c_str());
    GError *error = nullptr;
    host->backend = g_subprocess_launcher_spawn(launcher, &error, backend.c_str(), nullptr);
    g_object_unref(launcher);
    if (!host->backend) {
        g_printerr("Cannot start backend: %s\n", error->message); g_clear_error(&error);
        g_application_quit(G_APPLICATION(app)); return;
    }
    // Hold the application alive while waiting for the first window/handshake.
    host->output = g_data_input_stream_new(g_subprocess_get_stdout_pipe(host->backend));
    host->startupTimer = g_timeout_add_seconds(20, +[](gpointer data) -> gboolean {
        auto *h = static_cast<Host *>(data); h->startupTimer = 0;
        g_printerr("Backend did not report a port within 20 seconds\n");
        g_application_quit(G_APPLICATION(h->app)); return G_SOURCE_REMOVE;
    }, host);
    readBackend(host);
}
void stopBackend(GSubprocess *backend) {
    if (!backend) return;
    struct Wait { GMainLoop *loop; guint timer = 0; GSubprocess *process; };
    Wait wait{g_main_loop_new(nullptr, FALSE), 0, backend};
    g_subprocess_send_signal(backend, SIGTERM);
    wait.timer = g_timeout_add_seconds(2, +[](gpointer data) -> gboolean {
        auto *wait = static_cast<Wait *>(data); wait->timer = 0;
        g_subprocess_force_exit(wait->process); return G_SOURCE_REMOVE;
    }, &wait);
    g_subprocess_wait_async(backend, nullptr, +[](GObject *source, GAsyncResult *result, gpointer data) {
        auto *wait = static_cast<Wait *>(data);
        g_subprocess_wait_finish(G_SUBPROCESS(source), result, nullptr);
        g_main_loop_quit(wait->loop);
    }, &wait);
    g_main_loop_run(wait.loop);
    if (wait.timer) g_source_remove(wait.timer);
    g_main_loop_unref(wait.loop);
}
}
int main(int argc, char **argv) {
    Host host;
    for (int i = 1; i < argc; ++i) {
        if (strcmp(argv[i], "--minimized") == 0) host.minimized = true;
        else { g_printerr("Unknown GTK option: %s\n", argv[i]); return 2; }
    }
    char *self = g_file_read_link("/proc/self/exe", nullptr);
    host.executable = self ? self : argv[0]; g_free(self);
    const auto runtime = env("XDG_RUNTIME_DIR", "/tmp");
    int lock = open((runtime + "/openless.lock").c_str(), O_CREAT | O_RDWR | O_CLOEXEC, 0600);
    if (lock < 0) { perror("openless.lock"); return 1; }
    const auto socketPath = runtime + "/openless-host.sock";
    if (flock(lock, LOCK_EX | LOCK_NB) != 0) {
        close(lock);
        if (host.minimized) return 0;
        int fd = socket(AF_UNIX, SOCK_STREAM | SOCK_CLOEXEC, 0);
        sockaddr_un address{}; address.sun_family = AF_UNIX;
        if (socketPath.size() >= sizeof(address.sun_path)) { if (fd >= 0) close(fd); return 1; }
        memcpy(address.sun_path, socketPath.c_str(), socketPath.size() + 1);
        if (fd >= 0 && connect(fd, reinterpret_cast<sockaddr *>(&address), sizeof(address)) == 0) {
            send(fd, "show\n", 5, MSG_NOSIGNAL); close(fd); return 0;
        }
        if (fd >= 0) close(fd);
        g_printerr("OpenLess is running but its window endpoint is not ready; retry shortly\n"); return 1;
    }
    host.app = gtk_application_new("top.openless.OpenLess", G_APPLICATION_NON_UNIQUE);
    g_application_hold(G_APPLICATION(host.app));
    host.instance = g_socket_service_new();
    unlink(socketPath.c_str());
    auto *address = g_unix_socket_address_new(socketPath.c_str());
    GError *error = nullptr;
    if (!g_socket_listener_add_address(G_SOCKET_LISTENER(host.instance), address,
            G_SOCKET_TYPE_STREAM, G_SOCKET_PROTOCOL_DEFAULT, nullptr, nullptr, &error)) {
        g_printerr("Cannot bind window endpoint: %s\n", error->message); return 1;
    }
    g_object_unref(address); chmod(socketPath.c_str(), 0600);
    g_signal_connect(host.instance, "incoming", G_CALLBACK(+[](GSocketService *, GSocketConnection *connection, GObject *, gpointer data) -> gboolean {
        auto *h = static_cast<Host *>(data); g_object_ref(connection);
        g_input_stream_read_bytes_async(g_io_stream_get_input_stream(G_IO_STREAM(connection)), 64,
            G_PRIORITY_DEFAULT, h->cancel, +[](GObject *source, GAsyncResult *result, gpointer data) {
                auto *connection = G_SOCKET_CONNECTION(g_object_get_data(source, "openless-connection"));
                auto *h = static_cast<Host *>(data); GError *error = nullptr;
                auto *bytes = g_input_stream_read_bytes_finish(G_INPUT_STREAM(source), result, &error);
                if (bytes) {
                    gsize size; const auto *text = static_cast<const char *>(g_bytes_get_data(bytes, &size));
                    if (!h->stopping && std::string(text, size).find("show") != std::string::npos) {
                        h->showRequested = true;
                        if (h->windowsReady) show(h->surfaces[Main], true, true);
                    }
                    g_bytes_unref(bytes);
                }
                g_clear_error(&error); g_io_stream_close(G_IO_STREAM(connection), nullptr, nullptr); g_object_unref(connection);
            }, h);
        g_object_set_data(G_OBJECT(g_io_stream_get_input_stream(G_IO_STREAM(connection))), "openless-connection", connection);
        return TRUE;
    }), &host);
    g_socket_service_start(host.instance);
    g_signal_connect(host.app, "activate", G_CALLBACK(activate), &host);
    char *cleanArgs[] = {argv[0], nullptr};
    const int result = g_application_run(G_APPLICATION(host.app), 1, cleanArgs);
    host.stopping = true; g_cancellable_cancel(host.cancel); soup_session_abort(host.http);
    stopBackend(host.backend);
    unlink(socketPath.c_str()); close(lock);
    if (g_object_get_data(G_OBJECT(host.app), "openless-restart")) execl(host.executable.c_str(), host.executable.c_str(), nullptr);
    return host.windowsReady ? result : 1;
}
