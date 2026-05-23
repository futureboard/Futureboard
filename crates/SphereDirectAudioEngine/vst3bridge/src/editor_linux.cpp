// editor_linux.cpp — GTK4 + X11 IPlugView embedding for Linux
//
// VST3 on Linux uses kPlatformTypeX11EmbedWindowID ("XcbWindow"):
// the host provides an X11 Window ID; the plugin uses the XEmbed protocol
// to reparent its own window inside ours.
//
// Architecture:
//  - GTK4 and its GLib main loop run on a dedicated background thread
//    ("the GTK thread") started lazily on the first editor open.
//  - All GLib/GTK operations are executed on that thread via g_idle_add()
//    synchronised with a mutex + condvar so callers from the Electron / Node
//    thread can wait for results.
//  - The X11 XID is retrieved via GDK's X11 backend API after the window is
//    realized.  If the GDK backend is not X11 (e.g. Wayland), opening fails
//    with a clear error message — VST3 Wayland embedding is not yet standard.
//
// VST3 spec reference:
//   pluginterfaces/gui/iplugview.h — kPlatformTypeX11EmbedWindowID = "XcbWindow"

#include <condition_variable>
#include <cstdio>
#include <cstring>
#include <mutex>
#include <thread>

#include <gtk/gtk.h>

#ifdef GDK_WINDOWING_X11
#  include <gdk/x11/gdkx.h>
#endif
#ifdef GDK_WINDOWING_WAYLAND
#  include <gdk/wayland/gdkwayland.h>
#endif

#include "sphere_daux_editor_bridge.h"

// ── Forward declarations ──────────────────────────────────────────────────────

void close_editor_linux(SphereDauxVst3Processor* proc);
int  focus_editor_linux(SphereDauxVst3Processor* proc);

// ── GTK main-loop thread ──────────────────────────────────────────────────────

namespace {

GMainLoop*              s_main_loop    = nullptr;
std::mutex              s_init_mutex;
std::condition_variable s_init_cv;
bool                    s_gtk_ready    = false;

/// Entry point for the dedicated GTK event-loop thread.
void gtk_thread_main() {
    gtk_init(); // GTK4: no argc/argv

    {
        std::lock_guard<std::mutex> lk(s_init_mutex);
        s_main_loop = g_main_loop_new(nullptr, FALSE);
        s_gtk_ready = true;
    }
    s_init_cv.notify_all();

    g_main_loop_run(s_main_loop); // blocks until g_main_loop_quit()

    g_main_loop_unref(s_main_loop);
    s_main_loop = nullptr;
}

/// Ensure the GTK thread is running.  Blocks until GTK is fully initialised.
/// Returns true on success.
bool ensure_gtk() {
    static std::once_flag once;
    std::call_once(once, []() {
        std::thread(gtk_thread_main).detach();
    });

    std::unique_lock<std::mutex> lk(s_init_mutex);
    return s_init_cv.wait_for(lk, std::chrono::seconds(5),
                              [] { return s_gtk_ready; });
}

// ── Resize callback ────────────────────────────────────────────────────────

struct ResizeCtx {
    SphereDauxVst3Processor* proc;
};

#ifdef GDK_WINDOWING_X11
static void on_surface_layout(GdkSurface* /*surface*/,
                               int          width,
                               int          height,
                               gpointer     user_data) {
    auto* ctx = static_cast<ResizeCtx*>(user_data);
    if (ctx && ctx->proc) {
        sphere_daux_editor_notify_resize(ctx->proc, width, height);
    }
}
#endif

// ── Open-task (executed on the GTK thread via g_idle_add) ─────────────────

struct OpenTask {
    SphereDauxVst3Processor* proc{nullptr};
    const char*              window_id{nullptr};
    const char*              title{nullptr};
    int                      width{0};
    int                      height{0};

    unsigned long long       result{0};
    std::mutex               done_mutex;
    std::condition_variable  done_cv;
    bool                     done{false};
};

static gboolean idle_open_editor(gpointer user_data) {
    auto* task = static_cast<OpenTask*>(user_data);
    SphereDauxVst3Processor* proc = task->proc;

    // ── Create GtkWindow ──────────────────────────────────────────────────

    GtkWidget* window = gtk_window_new();

    const char* title_str = (task->title && *task->title) ? task->title : "Plugin Editor";
    gtk_window_set_title(GTK_WINDOW(window), title_str);
    gtk_window_set_default_size(GTK_WINDOW(window),
                                task->width  > 0 ? task->width  : 820,
                                task->height > 0 ? task->height : 560);
    gtk_window_set_resizable(GTK_WINDOW(window), TRUE);

    // Dark background via CSS provider
    GtkCssProvider* css = gtk_css_provider_new();
    gtk_css_provider_load_from_string(css,
        "window { background-color: #0b0f14; }");
    gtk_style_context_add_provider_for_display(
        gtk_widget_get_display(window),
        GTK_STYLE_PROVIDER(css),
        GTK_STYLE_PROVIDER_PRIORITY_APPLICATION);
    g_object_unref(css);

    // Realize the window to create the underlying GDK surface / X11 window
    // BEFORE querying the IPlugView preferred size (the plugin may need a
    // realised surface for DPI querying on some toolkits).
    gtk_widget_realize(window);

    // ── Query IPlugView preferred size & create the view ─────────────────

    int editor_width  = task->width  > 0 ? task->width  : 820;
    int editor_height = task->height > 0 ? task->height : 560;

    if (!sphere_daux_editor_create_view(proc, "XcbWindow",
                                        &editor_width, &editor_height)) {
        std::fprintf(stderr,
                     "[SphereVST3/linux] create_view('XcbWindow') failed\n");
        gtk_window_destroy(GTK_WINDOW(window));
        goto done;
    }

    gtk_window_set_default_size(GTK_WINDOW(window), editor_width, editor_height);

    // ── Get X11 Window ID ─────────────────────────────────────────────────

    {
        GdkSurface* surface = gtk_native_get_surface(GTK_NATIVE(window));

#ifdef GDK_WINDOWING_X11
        if (!GDK_IS_X11_SURFACE(surface)) {
#else
        if (true) {
#endif
            sphere_daux_editor_set_error(
                "DAUx VST3 editor: GDK backend is not X11 — "
                "VST3 XEmbed requires an X11 display");
            std::fprintf(stderr,
                         "[SphereVST3/linux] GDK backend is not X11; "
                         "cannot obtain XID for VST3 embed\n");
            sphere_daux_editor_detach_view(proc);
            gtk_window_destroy(GTK_WINDOW(window));
            goto done;
        }

#ifdef GDK_WINDOWING_X11
        // Ensure the X11 window exists (realize pushes creation to GDK
        // but the actual XCreateWindow may be deferred until first map on
        // some GDK versions; gdk_x11_surface_get_xid forces it).
        Window xid = gdk_x11_surface_get_xid(surface);
        std::fprintf(stderr,
                     "[SphereVST3/linux] X11 XID=0x%lx w=%d h=%d\n",
                     xid, editor_width, editor_height);

        // ── Attach IPlugView ──────────────────────────────────────────────
        // kPlatformTypeX11EmbedWindowID = "XcbWindow"
        // Pass the X11 Window ID cast to void*.
        if (!sphere_daux_editor_attach_view(proc,
                                            reinterpret_cast<void*>(
                                                static_cast<uintptr_t>(xid)),
                                            "XcbWindow")) {
            std::fprintf(stderr,
                         "[SphereVST3/linux] attach_view('XcbWindow') failed\n");
            gtk_window_destroy(GTK_WINDOW(window));
            goto done;
        }

        // ── Connect resize signal on the GDK surface ──────────────────────
        auto* resize_ctx = new ResizeCtx{proc};
        g_signal_connect_data(
            surface, "layout",
            G_CALLBACK(on_surface_layout),
            resize_ctx,
            [](gpointer data, GClosure*) { delete static_cast<ResizeCtx*>(data); },
            G_CONNECT_DEFAULT);

        // ── Store native state ─────────────────────────────────────────────
        unsigned long long handle = sphere_daux_editor_next_handle();

        // We keep a strong reference to the GtkWidget via g_object_ref.
        g_object_ref(window);

        sphere_daux_editor_store_native(
            proc,
            window,      // native_window  (GtkWidget*, g_object_ref'd)
            nullptr,     // native_embed   (unused on Linux)
            nullptr,     // native_delegate
            handle,
            task->window_id ? task->window_id : "",
            title_str,
            task->width, task->height);

        // ── Close signal: user pressed the window X button ────────────────
        // We use a lambda-style GCallback via g_signal_connect_data with a
        // heap-allocated copy of the processor pointer.
        struct CloseCtx { SphereDauxVst3Processor* proc; };
        auto* close_ctx = new CloseCtx{proc};
        g_signal_connect_data(
            window, "close-request",
            G_CALLBACK(+[](GtkWindow*, gpointer ud) -> gboolean {
                auto* ctx = static_cast<CloseCtx*>(ud);
                if (ctx->proc) close_editor_linux(ctx->proc);
                return TRUE; // prevent default GTK close behaviour
            }),
            close_ctx,
            [](gpointer data, GClosure*) { delete static_cast<CloseCtx*>(data); },
            G_CONNECT_DEFAULT);

        // ── Show the window ───────────────────────────────────────────────
        gtk_window_present(GTK_WINDOW(window));

        std::fprintf(stderr,
                     "[SphereVST3/linux] editor opened handle=%llu "
                     "xid=0x%lx windowId=%s\n",
                     handle, xid,
                     task->window_id ? task->window_id : "");

        task->result = handle;
#endif // GDK_WINDOWING_X11
    }

done:
    {
        std::lock_guard<std::mutex> lk(task->done_mutex);
        task->done = true;
    }
    task->done_cv.notify_all();
    return G_SOURCE_REMOVE;
}

// ── Close-task ────────────────────────────────────────────────────────────

struct CloseTask {
    SphereDauxVst3Processor* proc{nullptr};
    std::mutex               done_mutex;
    std::condition_variable  done_cv;
    bool                     done{false};
};

static gboolean idle_close_editor(gpointer user_data) {
    auto* task = static_cast<CloseTask*>(user_data);
    SphereDauxVst3Processor* proc = task->proc;

    void* win_ptr = sphere_daux_editor_get_native_window(proc);
    if (win_ptr) {
        sphere_daux_editor_clear_native(proc);   // zero first to prevent re-entrancy
        sphere_daux_editor_detach_view(proc);    // IPlugView::removed()

        GtkWidget* window = static_cast<GtkWidget*>(win_ptr);
        gtk_window_destroy(GTK_WINDOW(window));
        g_object_unref(window); // matches the g_object_ref in idle_open_editor

        std::fprintf(stderr, "[SphereVST3/linux] editor closed\n");
    }

    {
        std::lock_guard<std::mutex> lk(task->done_mutex);
        task->done = true;
    }
    task->done_cv.notify_all();
    return G_SOURCE_REMOVE;
}

} // namespace

// ── Public platform functions ─────────────────────────────────────────────────

unsigned long long open_editor_linux(
    SphereDauxVst3Processor* proc,
    const char*              window_id,
    const char*              title,
    int                      width,
    int                      height)
{
    if (!proc) return 0;

    if (!ensure_gtk()) {
        sphere_daux_editor_set_error("GTK4 initialisation timed out");
        std::fprintf(stderr, "[SphereVST3/linux] GTK4 init failed\n");
        return 0;
    }

    // Already open? Focus it.
    if (sphere_daux_editor_get_native_window(proc)) {
        return focus_editor_linux(proc) ? sphere_daux_editor_get_handle(proc) : 0;
    }

    OpenTask task;
    task.proc      = proc;
    task.window_id = window_id;
    task.title     = title;
    task.width     = width;
    task.height    = height;

    g_idle_add(idle_open_editor, &task);

    std::unique_lock<std::mutex> lk(task.done_mutex);
    task.done_cv.wait(lk, [&] { return task.done; });
    return task.result;
}

void close_editor_linux(SphereDauxVst3Processor* proc) {
    if (!proc) return;
    if (!sphere_daux_editor_get_native_window(proc)) return;
    if (!s_gtk_ready) return;

    CloseTask task;
    task.proc = proc;

    g_idle_add(idle_close_editor, &task);

    std::unique_lock<std::mutex> lk(task.done_mutex);
    task.done_cv.wait(lk, [&] { return task.done; });
}

int focus_editor_linux(SphereDauxVst3Processor* proc) {
    if (!proc || !s_gtk_ready) return 0;
    void* win_ptr = sphere_daux_editor_get_native_window(proc);
    if (!win_ptr) return 0;

    g_idle_add([](gpointer ud) -> gboolean {
        auto* window = static_cast<GtkWidget*>(ud);
        gtk_window_present(GTK_WINDOW(window));
        return G_SOURCE_REMOVE;
    }, win_ptr);
    return 1;
}

void shutdown_editor_linux(SphereDauxVst3Processor* proc) {
    close_editor_linux(proc);
}
