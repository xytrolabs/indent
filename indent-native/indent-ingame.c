/*
 * indent-ingame — native canvas game window for Indent's InGame framework.
 *
 * Indent drives a game loop and writes each frame as JSON to <workdir>/frame.json:
 *   {"clear":"#000000","shapes":[
 *     {"t":"rect","x":0,"y":0,"w":20,"h":20,"c":"#39d353"},
 *     {"t":"circle","cx":10,"cy":10,"r":5,"c":"#f85149"},
 *     {"t":"text","x":4,"y":12,"s":"Score: 10","c":"#fff","size":14}]}
 *
 * This helper polls frame.json; on change it renders the frame to a canvas via
 * a WebKitGTK JS bridge. Keyboard input is appended as JSON lines to
 * <workdir>/events.txt:
 *   {"key":"ArrowUp","down":true}
 *   {"type":"quit"}
 *
 * Usage: indent-ingame <workdir> <title> <width> <height>
 */
#include <gtk/gtk.h>
#include <webkit2/webkit2.h>
#include <glib.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <time.h>

static char *workdir = NULL;
static GtkWidget *webview = NULL;
static char *last_frame = NULL;

static char *read_file(const char *path) {
    FILE *f = fopen(path, "r");
    if (!f) return NULL;
    fseek(f, 0, SEEK_END);
    long sz = ftell(f);
    fseek(f, 0, SEEK_SET);
    if (sz < 0) { fclose(f); return NULL; }
    char *buf = malloc(sz + 1);
    if (!buf) { fclose(f); return NULL; }
    size_t rd = fread(buf, 1, sz, f);
    buf[rd] = '\0';
    fclose(f);
    return buf;
}

static void append_event(const char *line) {
    char path[1024];
    snprintf(path, sizeof path, "%s/events.txt", workdir);
    FILE *f = fopen(path, "a");
    if (!f) return;
    fputs(line, f);
    fclose(f);
}

static const char *key_name(guint keyval) {
    switch (keyval) {
        case GDK_KEY_Up: return "ArrowUp";
        case GDK_KEY_Down: return "ArrowDown";
        case GDK_KEY_Left: return "ArrowLeft";
        case GDK_KEY_Right: return "ArrowRight";
        case GDK_KEY_space: return " ";
        case GDK_KEY_Return: return "Enter";
        case GDK_KEY_Escape: return "Escape";
        case GDK_KEY_BackSpace: return "Backspace";
        case GDK_KEY_Tab: return "Tab";
        default: {
            static char buf[8];
            gchar *name = gdk_keyval_name(keyval);
            if (name && strlen(name) == 1 && g_ascii_isalnum(name[0])) {
                snprintf(buf, sizeof buf, "%s", name);
                return buf;
            }
            return NULL;
        }
    }
}

static gboolean on_key_press(GtkWidget *w, GdkEventKey *e, gpointer d) {
    const char *kn = key_name(e->keyval);
    if (kn) {
        char line[512];
        snprintf(line, sizeof line, "{\"key\":\"%s\",\"down\":true}\n", kn);
        append_event(line);
    }
    return FALSE;
}

static gboolean on_key_release(GtkWidget *w, GdkEventKey *e, gpointer d) {
    const char *kn = key_name(e->keyval);
    if (kn) {
        char line[512];
        snprintf(line, sizeof line, "{\"key\":\"%s\",\"down\":false}\n", kn);
        append_event(line);
    }
    return FALSE;
}

static void on_destroy(GtkWidget *w, gpointer d) {
    append_event("{\"type\":\"quit\"}\n");
    gtk_main_quit();
}

static gboolean poll_frame(gpointer data) {
    if (!webview) return TRUE;
    char path[1024];
    snprintf(path, sizeof path, "%s/frame.json", workdir);

    char *frame = read_file(path);
    if (frame) {
        int changed = (!last_frame) || strcmp(frame, last_frame) != 0;
        if (changed) {
            // Pass the JSON object to the page's draw() function.
            // JSON is valid JS, so wrap it as draw(<json>).
            GString *js = g_string_new("draw(");
            g_string_append(js, frame);
            g_string_append(js, ");");
            webkit_web_view_run_javascript(WEBKIT_WEB_VIEW(webview), js->str,
                NULL, NULL, NULL);
            g_string_free(js, TRUE);

            free(last_frame);
            last_frame = frame;
        } else {
            free(frame);
        }
    }
    return TRUE; // keep polling
}

static void on_load_changed(WebKitWebView *wv, WebKitLoadEvent ev, gpointer d) {
    // Start polling once the page is loaded.
    static guint timer_id = 0;
    if (ev == WEBKIT_LOAD_FINISHED && timer_id == 0) {
        timer_id = g_timeout_add(33, poll_frame, NULL); // ~30 fps poll
    }
}

int main(int argc, char **argv) {
    if (argc < 5) {
        fprintf(stderr, "usage: indent-ingame <workdir> <title> <width> <height>\n");
        return 1;
    }
    workdir = argv[1];
    const char *title = argv[2];
    int width = atoi(argv[3]);
    int height = atoi(argv[4]);

    gtk_init(&argc, &argv);

    GtkWidget *window = gtk_window_new(GTK_WINDOW_TOPLEVEL);
    gtk_window_set_title(GTK_WINDOW(window), title);
    gtk_window_set_default_size(GTK_WINDOW(window), width, height);
    g_signal_connect(window, "destroy", G_CALLBACK(on_destroy), NULL);
    g_signal_connect(window, "key-press-event", G_CALLBACK(on_key_press), NULL);
    g_signal_connect(window, "key-release-event", G_CALLBACK(on_key_release), NULL);

    webview = webkit_web_view_new();
    gtk_container_add(GTK_CONTAINER(window), webview);

    // Page with a canvas and a draw() function that renders Indent's frame JSON.
    const char *page =
        "<!DOCTYPE html><html><body style='margin:0;background:#000'>"
        "<canvas id='c'></canvas>"
        "<script>"
        "var c=document.getElementById('c');"
        "var ctx=c.getContext('2d');"
        "function fit(){"
        "  c.width=window.innerWidth; c.height=window.innerHeight;"
        "  ctx.fillStyle='#000'; ctx.fillRect(0,0,c.width,c.height);"
        "}"
        "window.onresize=fit;"
        "window.draw=function(d){"
        "  ctx.fillStyle=d.clear||'#000'; ctx.fillRect(0,0,c.width,c.height);"
        "  var sh=d.shapes||[];"
        "  for(var i=0;i<sh.length;i++){"
        "    var s=sh[i];"
        "    if(s.t==='rect'){ctx.fillStyle=s.c;ctx.fillRect(s.x,s.y,s.w,s.h);}"
        "    else if(s.t==='circle'){ctx.fillStyle=s.c;ctx.beginPath();"
        "      ctx.arc(s.cx,s.cy,s.r,0,Math.PI*2);ctx.fill();}"
        "    else if(s.t==='text'){ctx.fillStyle=s.c;"
        "      ctx.font=(s.size||14)+'px monospace';ctx.fillText(s.s,s.x,s.y);}"
        "  }"
        "};"
        "fit();draw({'clear':'#000','shapes':[]});"
        "</script></body></html>";

    webkit_web_view_load_html(WEBKIT_WEB_VIEW(webview), page, NULL);
    g_signal_connect(webview, "load-changed", G_CALLBACK(on_load_changed), NULL);

    gtk_widget_show_all(window);
    gtk_main();
    return 0;
}
