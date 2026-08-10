/*
 * indent-ingame — native canvas game window for Indent's InGame framework.
 *
 * PyGame-style: Indent drives the game loop; this window draws JSON frames
 * and reports input.
 *
 * Indent writes each frame to <workdir>/frame.json:
 *   {"clear":"#000000","shapes":[
 *     {"t":"rect","x":0,"y":0,"w":20,"h":20,"c":"#39d353","rot":45},
 *     {"t":"circle","cx":10,"cy":10,"r":5,"c":"#f85149"},
 *     {"t":"ellipse","cx":10,"cy":10,"rx":8,"ry":4,"c":"#58a6ff"},
 *     {"t":"arc","cx":50,"cy":50,"r":20,"a1":0,"a2":120,"c":"#d29922"},
 *     {"t":"line","x1":0,"y1":0,"x2":100,"y2":100,"c":"#58a6ff","w":2},
 *     {"t":"polygon","pts":[[0,0],[10,0],[5,10]],"c":"#d29922"},
 *     {"t":"sprite","x":4,"y":4,"w":20,"h":20,"s":"\u26ab","size":16},
 *     {"t":"text","x":4,"y":12,"s":"Score: 10","c":"#fff","size":14}]}
 *
 * This helper polls frame.json; on change it renders via a WebKitGTK JS bridge.
 * Input is reported to <workdir>/events.txt as JSON lines:
 *   {"key":"ArrowUp","down":true}
 *   {"mousemove":true,"x":123,"y":45}
 *   {"mousedown":true,"button":1,"x":123,"y":45}
 *   {"type":"quit"}
 * Current held-key state is written to <workdir>/keys.txt (JSON list) and the
 * mouse position to <workdir>/mouse.txt (JSON [x, y]) on every change.
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

static char *workdir = NULL;
static GtkWidget *webview = NULL;
static char *last_frame = NULL;
static GHashTable *pressed = NULL;   // keyval -> gpointer
static int mouse_x = 0, mouse_y = 0;
static guint mouse_buttons = 0;

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

static void write_keys(void) {
    char path[1024];
    snprintf(path, sizeof path, "%s/keys.txt", workdir);
    FILE *f = fopen(path, "w");
    if (!f) return;
    fputs("[", f);
    GList *keys = g_hash_table_get_keys(pressed);
    GList *it = keys;
    int first = 1;
    while (it) {
        if (!first) fputs(",", f);
        fputs("\"", f);
        fputs((char *)it->data, f);
        fputs("\"", f);
        first = 0;
        it = it->next;
    }
    fputs("]", f);
    fclose(f);
    g_list_free(keys);
}

static void write_mouse(void) {
    char path[1024];
    snprintf(path, sizeof path, "%s/mouse.txt", workdir);
    FILE *f = fopen(path, "w");
    if (!f) return;
    fprintf(f, "[%d,%d]", mouse_x, mouse_y);
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
        if (!g_hash_table_contains(pressed, kn)) {
            g_hash_table_add(pressed, g_strdup(kn));
            write_keys();
        }
        char line[512];
        snprintf(line, sizeof line, "{\"key\":\"%s\",\"down\":true}\n", kn);
        append_event(line);
    }
    return FALSE;
}

static gboolean on_key_release(GtkWidget *w, GdkEventKey *e, gpointer d) {
    const char *kn = key_name(e->keyval);
    if (kn) {
        if (g_hash_table_contains(pressed, kn)) {
            g_hash_table_remove(pressed, kn);
            write_keys();
        }
        char line[512];
        snprintf(line, sizeof line, "{\"key\":\"%s\",\"down\":false}\n", kn);
        append_event(line);
    }
    return FALSE;
}

static gboolean on_motion(GtkWidget *w, GdkEventMotion *e, gpointer d) {
    mouse_x = (int)e->x;
    mouse_y = (int)e->y;
    write_mouse();
    char line[512];
    snprintf(line, sizeof line, "{\"mousemove\":true,\"x\":%d,\"y\":%d}\n", mouse_x, mouse_y);
    append_event(line);
    return FALSE;
}

static gboolean on_button(GtkWidget *w, GdkEventButton *e, gpointer d) {
    mouse_x = (int)e->x;
    mouse_y = (int)e->y;
    write_mouse();
    char line[512];
    if (e->type == GDK_BUTTON_PRESS) {
        snprintf(line, sizeof line, "{\"mousedown\":true,\"button\":%d,\"x\":%d,\"y\":%d}\n", e->button, mouse_x, mouse_y);
    } else {
        snprintf(line, sizeof line, "{\"mouseup\":true,\"button\":%d,\"x\":%d,\"y\":%d}\n", e->button, mouse_x, mouse_y);
    }
    append_event(line);
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
    return TRUE;
}

static void on_load_changed(WebKitWebView *wv, WebKitLoadEvent ev, gpointer d) {
    static guint timer_id = 0;
    if (ev == WEBKIT_LOAD_FINISHED && timer_id == 0) {
        timer_id = g_timeout_add(33, poll_frame, NULL);
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

    pressed = g_hash_table_new_full(g_str_hash, g_str_equal, g_free, NULL);

    gtk_init(&argc, &argv);

    GtkWidget *window = gtk_window_new(GTK_WINDOW_TOPLEVEL);
    gtk_window_set_title(GTK_WINDOW(window), title);
    gtk_window_set_default_size(GTK_WINDOW(window), width, height);
    g_signal_connect(window, "destroy", G_CALLBACK(on_destroy), NULL);
    g_signal_connect(window, "key-press-event", G_CALLBACK(on_key_press), NULL);
    g_signal_connect(window, "key-release-event", G_CALLBACK(on_key_release), NULL);
    g_signal_connect(window, "motion-notify-event", G_CALLBACK(on_motion), NULL);
    g_signal_connect(window, "button-press-event", G_CALLBACK(on_button), NULL);
    g_signal_connect(window, "button-release-event", G_CALLBACK(on_button), NULL);

    gtk_widget_add_events(window, GDK_POINTER_MOTION_MASK | GDK_BUTTON_PRESS_MASK | GDK_BUTTON_RELEASE_MASK);

    webview = webkit_web_view_new();
    gtk_container_add(GTK_CONTAINER(window), webview);

    // Canvas page: draw() renders Indent's frame JSON (rect/circle/line/polygon/text).
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
        "    if(s.t==='rect'){"
        "      ctx.save();ctx.translate(s.x+s.w/2,s.y+s.h/2);"
        "      if(s.rot)ctx.rotate(s.rot*Math.PI/180);"
        "      ctx.fillStyle=s.c;ctx.fillRect(-s.w/2,-s.h/2,s.w,s.h);"
        "      ctx.restore();}"
        "    else if(s.t==='circle'){ctx.fillStyle=s.c;ctx.beginPath();"
        "      ctx.arc(s.cx,s.cy,s.r,0,Math.PI*2);ctx.fill();}"
        "    else if(s.t==='ellipse'){ctx.fillStyle=s.c;ctx.beginPath();"
        "      ctx.ellipse(s.cx,s.cy,s.rx,s.ry,0,0,Math.PI*2);ctx.fill();}"
        "    else if(s.t==='arc'){ctx.fillStyle=s.c;ctx.beginPath();"
        "      ctx.moveTo(s.cx,s.cy);"
        "      ctx.arc(s.cx,s.cy,s.r,(s.a1||0)*Math.PI/180,(s.a2||360)*Math.PI/180);"
        "      ctx.closePath();ctx.fill();}"
        "    else if(s.t==='line'){ctx.strokeStyle=s.c;ctx.lineWidth=s.w||2;"
        "      ctx.beginPath();ctx.moveTo(s.x1,s.y1);ctx.lineTo(s.x2,s.y2);ctx.stroke();}"
        "    else if(s.t==='polygon'){ctx.fillStyle=s.c;ctx.beginPath();"
        "      var p=s.pts;ctx.moveTo(p[0][0],p[0][1]);"
        "      for(var k=1;k<p.length;k++){ctx.lineTo(p[k][0],p[k][1]);}"
        "      ctx.closePath();ctx.fill();}"
        "    else if(s.t==='sprite'){ctx.fillStyle=s.c||'#fff';"
        "      ctx.font=(s.size||s.h||16)+'px serif';"
        "      ctx.textAlign='center';ctx.textBaseline='middle';"
        "      ctx.fillText(s.s,s.x+(s.w||0)/2,s.y+(s.h||0)/2);"
        "      ctx.textAlign='start';ctx.textBaseline='alphabetic';}"
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
