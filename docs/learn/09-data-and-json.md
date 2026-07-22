# Lesson 09 — Data: JSON, HTTP, Time & Random

**Time**: 30 minutes  |  **Topics**: JSON, HTTP requests, WebSocket, Time, Random numbers, Math

---

## 🎯 Learning Objectives
- Parse and generate JSON
- Make HTTP GET/POST/PUT/DELETE requests
- Work with real-time WebSocket connections
- Use timestamps, delays, and high-resolution timers
- Generate random numbers and shuffle data

---

## JSON

JSON is the most common data format for APIs and config files. Indent has built-in helpers.

### Parsing JSON

```indent
var json_text string = '{"name": "Ada", "age": 28, "skills": ["math", "poetry"]}'
var data dynamic = json_loads json_text

say data.name             #! "Ada" (dot notation)
say data["age"]           #! 28
say data.skills[0]        #! "math"
```

### Serializing to JSON

```indent
var person dynamic = {"name": "Indent", "version": 2, "features": ["simple", "fast"]}
var json_string string = json_dumps person
say json_string           #! {"name": "Indent", "version": 2, "features": ["simple", "fast"]}
```

### Loading JSON from a file

```indent
#! config.json: {"theme": "dark", "port": 8080, "debug": false}
if os_exists "config.json"
    var raw string = file_read_text "config.json"
    var config dynamic = json_loads raw
    say "Theme: " + config.theme
    say "Port: " + config.port
```

### Saving JSON to a file

```indent
var settings dynamic = {"volume": 75, "fullscreen": true, "resolution": "1920x1080"}
var json string = json_dumps settings
file_write_text "settings.json" json
```

---

## HTTP Requests

### GET request

```indent
var response dynamic = http_get "https://api.github.com/repos/xytrolabs/indent"
say "Stars: " + response.stargazers_count
say "Description: " + response.description
```

With authorization:

```indent
var authed dynamic = http_get "https://api.github.com/user" "Bearer ghp_your_token"
say "Login: " + authed.login
```

### POST request (JSON body)

```indent
var payload dynamic = {"title": "Bug report", "body": "Something broke"}
var result dynamic = http_post_json "https://api.example.com/issues" payload "Bearer token123"
say "Created: " + result.id
```

### PUT, PATCH, DELETE

```indent
#! Update a resource
http_put_json "https://api.example.com/items/1" {"name": "Updated"} "Bearer token"

#! Partial update
http_patch_json "https://api.example.com/items/1" {"status": "done"} "Bearer token"

#! Delete a resource
http_delete "https://api.example.com/items/1" "Bearer token"
```

### Full API client example

```indent
fun fetch_user user_id
    var url string = "https://jsonplaceholder.typicode.com/users/" + user_id
    var response dynamic = http_get url
    give response

var user dynamic = fetch_user 1
say "Name: " + user.name
say "Email: " + user.email
say "City: " + user.address.city
```

---

## WebSocket

Connect to real-time services:

```indent
var ws dynamic = ws_connect "wss://echo.example.com"
ws_send_text ws.socket_id "Hello!"
var reply string = ws_recv_text ws.socket_id
say "Echo: " + reply
ws_close ws.socket_id

#! With timeout
ws_send_text ws.socket_id "Ping?"
var resp string = ws_recv_text_timeout ws.socket_id 5.0
if is_missing resp
    say "No response in 5 seconds"
otherwise
    say "Response: " + resp
```

---

## Time

### Current timestamp

```indent
var now float = time_now
say "Current Unix timestamp: " + now       #! e.g. 1749600000.0
```

### Sleeping / delays

```indent
say "Starting..."
time_sleep 1.5
say "1.5 seconds later..."
```

### High-resolution timer (for benchmarking)

```indent
var start float = time_perf_counter
#! ... do some work ...
repeat 10000
    say ""
var elapsed float = time_perf_counter - start
say "Took " + elapsed + " seconds"
```

### Using the `datetime` module

```indent
get Now from datetime
get Format from datetime

var now_string string = Now
say "Current time: " + now_string

var formatted string = Format "%Y-%m-%d %H:%M:%S"
say "Formatted: " + formatted
```

---

## Random

### Random numbers

```indent
get RandInt from random
get Random from random

#! Random integer between 1 and 100 (inclusive)
var dice int = RandInt 1 100
say "You rolled: " + dice

#! Random float between 0.0 and 1.0
var r float = Random
say r
```

### Random choice from a list

```indent
get Choice from random
get Shuffle from random

var options dynamic = ["rock", "paper", "scissors"]
var pick string = Choice options
say "Computer chose: " + pick

#! Shuffle a list
Shuffle options
say options               #! items in random order
```

### Seeded randomness (reproducible)

```indent
get Seed from random

Seed 42
say RandInt 1 100         #! Always the same
say RandInt 1 100         #! Always the same sequence
```

### Random password generator

```indent
fun generate_password length
    var chars string = "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789!@#$%"
    var result string = ""
    repeat length
        var index int = RandInt 0 len(chars) - 1
        result is result + chars[index]
    give result

say generate_password 16
```

---

## Putting It All Together

A weather report script:

```indent
get RandInt from random

#! Fetch weather data (simulated with a free API)
var city string = "London"
var url string = "https://wttr.in/" + city + "?format=j1"
var response dynamic = http_get url

var current_condition dynamic = response.current_condition[0]
say "Weather in " + city + ":"
say "  Temperature: " + current_condition.temp_C + "°C"
say "  Humidity: " + current_condition.humidity + "%"
say "  Description: " + current_condition.weatherDesc[0].value

#! Log the query
var log_entry dynamic = {
    "city": city,
    "temp": current_condition.temp_C,
    "timestamp": time_now
}
file_append_text "weather_log.json" json_dumps(log_entry) + "\n"
```

---

## WebSocket Receive & Close

```indent
var ws_id int = ws_connect "wss://echo.websocket.org"
ws_send_text ws_id "Hello!"

# Receive with timeout (seconds)
var msg string = ws_recv_text_timeout ws_id 5.0
say "Received: " + msg

# Non-blocking receive
var msg2 string = ws_recv_text ws_id

ws_close ws_id
```

## Math Functions (Extended)

```indent
# Advanced trig
say math_asin 0.5       #! 0.523... (arcsin in radians)
say math_acos 0.5       #! 1.047... (arccos)
say math_atan 1.0       #! 0.785... (arctan)
say math_atan2 1 1      #! 0.785... (2-arg arctan)

# Logs and exponents
say math_log 8 2        #! 3.0  (log base 2 of 8)
say math_log10 100      #! 2.0  (log base 10)
say math_exp 2          #! 7.389... (e^2)

# Rounding
say math_round 3.14159 2   #! 3.14
say math_round 3.14159 0   #! 3.0
say math_floor 3.9          #! 3.0
say math_ceil 3.1           #! 4.0
```

## High-Resolution Timing

```indent
# time_perf_counter — monotonic timer, good for benchmarks
var start float = time_perf_counter()
# ... do work ...
var elapsed float = time_perf_counter() - start
say "Took " + elapsed + " seconds"
```

## Practice

1. Fetch data from a public API and print specific fields.
2. Build a JSON config loader that merges user settings with defaults.
3. Write a dice-rolling simulator that uses `RandInt` and tracks statistics.
4. Create a timer that counts down from N seconds, printing remaining time each second.
5. Save a list of random passwords to a file in JSON format.

---

## 📖 Next Lesson
→ [Lesson 10: Web Development](10-web-development.md)
