# Lesson 10 — Web Development with Indent

**Time**: 35 minutes  |  **Topics**: HTML generation, Static sites, HTTP server, JSON APIs

---

## 🎯 Learning Objectives
- Generate HTML pages with the `html` package
- Build a static site generator
- Serve a website with Indent's built-in HTTP server
- Fetch data from web APIs

---

## The `html` Package

Indent comes with a built-in HTML generation package. Import it and start building:

```indent
get Tag from html
get Page from html
get Escape from html

# Build a simple heading
say Tag("h1", {}, "Welcome to Indent")
# Output: <h1>Welcome to Indent</h1>

# Build a div with a CSS class
say Tag("div", {"class": "hero"}, "Hello World")
# Output: <div class="hero">Hello World</div>
```

### Full Page

```indent
get Page from html
get Tag from html
get Paragraph from html

var body string = ""
body is body + Tag("h1", {}, "My Indent Site")
body is body + Paragraph({}, "Built with Indent, a simple programming language.")

var page string = Page("My Site", "", body)
file_write_text("index.html", page)

say "Site generated: index.html"
```

### HTML Escaping

Always escape user input to prevent XSS:

```indent
get Escape from html

var user_input string = "<script>alert('xss')</script>"
var safe string = Escape(user_input)
say safe   # &lt;script&gt;alert(&#39;xss&#39;)&lt;/script&gt;
```

---

## Static Site Generator

Let's build a simple static site generator that converts basic Markdown to HTML:

```indent
get Tag from html
get Page from html
get Escape from html

fun MarkdownToHtml md
    var html string = ""
    var lines dynamic = split(md, "\n")
    repeat line in lines
        var t string = trim(line)
        if len(t) == 0
            html is html + "\n"
        otherwise
            var p2 string = slice(t, 0, 2)
            if p2 == "# "
                html is html + Tag("h1", {}, Escape(slice(t, 2, 9999))) + "\n"
            or p2 == "## "
                html is html + Tag("h2", {}, Escape(slice(t, 3, 9999))) + "\n"
            or p2 == "- "
                html is html + Tag("li", {}, Escape(slice(t, 2, 9999))) + "\n"
            otherwise
                html is html + Tag("p", {}, Escape(t)) + "\n"
    give html

# Use it!
var md string = "# Hello\n\nThis is a paragraph.\n\n- Item 1\n- Item 2"
var body string = MarkdownToHtml(md)
var page string = Page("My Site", "", body)
file_write_text("public/index.html", page)
say "Built public/index.html"
```

---

## HTTP Server

Indent can serve a directory of static files:

```indent
# Serve the current directory on port 8080
http_serve_dir(".", 8080)
say "Serving on http://localhost:8080"
```

Visit `http://localhost:8080` in your browser to see your site!

For the SSG workflow:
1. Generate HTML with the `html` package → `public/` directory
2. Serve with `http_serve_dir("public", 8080)`
3. Browse your site locally

---

## Fetching Data from APIs

Indent has a built-in HTTP client:

```indent
# GET request
var response dynamic = http_get("https://api.github.com")
say response["status"]
say response["body"]

# Parse JSON response
var data dynamic = json_loads(response["body"])
say data["current_user_url"]
```

### Building an API-powered page

```indent
get Tag from html
get Page from html

# Fetch data
var resp dynamic = http_get("https://api.github.com/repos/xytrolabs/indent")
var repo dynamic = json_loads(resp["body"])

# Build HTML
var content string = ""
content is content + Tag("h1", {}, repo["full_name"])
content is content + Tag("p", {}, repo["description"])
content is content + Tag("p", {}, "⭐ " + string(repo["stargazers_count"]) + " stars")

var page string = Page("Indent on GitHub", "", content)
file_write_text("github.html", page)
say "Built github.html"
```

---

## Real-World Project: Personal Blog

Let's put it all together — a personal blog with:
- Blog posts stored as `.md` files in `content/`
- Generated HTML output in `public/`
- A local dev server

```indent
#! blog.ind — Personal blog generator

get Tag from html
get Page from html
get Escape from html

# Markdown-to-HTML converter (from earlier)
fun MarkdownToHtml md
    var html string = ""
    var lines dynamic = split(md, "\n")
    repeat line in lines
        var t string = trim(line)
        if len(t) == 0
            html is html + "\n"
        otherwise
            var p2 string = slice(t, 0, 2)
            if p2 == "# "
                html is html + Tag("h1", {}, Escape(slice(t, 2, 9999))) + "\n"
            or p2 == "## "
                html is html + Tag("h2", {}, Escape(slice(t, 3, 9999))) + "\n"
            otherwise
                html is html + Tag("p", {}, Escape(t)) + "\n"
    give html

# Build a single post
fun BuildPost path
    var md string = file_read_text(path)
    var body string = MarkdownToHtml(md)
    give Page("My Blog", "", body)

# Build all posts
var posts dynamic = os_list_dir("content")
repeat post in posts
    if ends_with(post, ".md")
        var body string = BuildPost("content/" + post)
        var outName string = replace(post, ".md", ".html")
        file_write_text("public/" + outName, body)
        say "Built: public/" + outName

say "Blog generated! Run: indent -e 'http_serve_dir(\"public\", 8080)'"
```

---

## 🎯 Exercises

### Exercise 1: Portfolio Page
Create a personal portfolio page with:
- Your name as an `<h1>`
- A short bio in a `<p>`
- A list of skills using `<ul>`/`<li>`
Save it as `portfolio.html`.

### Exercise 2: Weather Page
Fetch weather data from an API and display it as an HTML page. Use `https://api.open-meteo.com/v1/forecast?latitude=52.52&longitude=13.41&current_weather=true` (free, no API key needed).

### Exercise 3: Blog Engine
Extend the blog project above to:
- Generate an index page listing all posts
- Add a navigation bar
- Support `###` sub-headings in Markdown

### 🔥 Challenge: Live Reload
Build a script that regenerates your site every 5 seconds while the server is running. Use `repeat` with `time_sleep()` and re-run the build logic each iteration.

---

## 📖 Next Lesson
→ [Lesson 11: Classes & Objects](11-classes.md)
