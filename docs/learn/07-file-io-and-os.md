# Lesson 07 — File I/O & OS

**Time**: 20 minutes  |  **Topics**: File read/write, Directories, Environment variables, System commands

---

## 🎯 Learning Objectives
- Read, write, and append files
- Navigate the filesystem: list, create, delete, rename
- Work with environment variables and paths
- Run system commands from Indent

---

## Reading and Writing Files

### Write text to a file

```indent
file_write_text "/tmp/hello.txt" "Hello, Indent!"
```

### Read text from a file

```indent
var content string = file_read_text "/tmp/hello.txt"
say content               #! "Hello, Indent!"
```

### Append to a file

```indent
file_append_text "/tmp/log.txt" "Line 1\n"
file_append_text "/tmp/log.txt" "Line 2\n"
```

---

## Filesystem Operations

### Check if something exists

```indent
var path string = "/tmp/data.txt"

if os_exists path
    say path + " exists"
otherwise
    say path + " does not exist"

#! Check if it's a file or directory
if os_is_file path
    say "It's a file"
if os_is_dir path
    say "It's a directory"
```

### Create and remove directories

```indent
#! Create a directory
os_mkdir "/tmp/my_project"

#! Create nested directories
os_mkdir "/tmp/a/b/c"

#! List contents of a directory
var entries dynamic = os_list_dir "/tmp"
repeat entry in entries
    say entry
```

### Delete and rename

```indent
#! Delete a file
os_remove "/tmp/old.txt"

#! Rename or move
os_rename "/tmp/source.txt" "/tmp/dest.txt"
```

---

## Environment Variables

### Reading environment variables

```indent
#! Get with a default fallback
var home string = os_getenv "HOME" "/tmp"
var port int = int_or os_getenv "PORT" "8080"
var debug boolean = bool os_getenv "DEBUG" "false"

say "Home: " + home
say "Port: " + port
```

### Setting environment variables

```indent
os_setenv "MY_VAR" "hello"
say os_getenv "MY_VAR" "default"      #! "hello"
```

### View all environment variables

```indent
var env dynamic = os_environ
repeat pair in items env
    say pair[0] + " = " + pair[1]
```

---

## Running System Commands

```indent
#! Run a command and get its output
var result dynamic = os_system "ls -la /tmp"
say result
```

---

## Working with Paths

### Get current directory

```indent
var cwd string = os_getcwd
say "You are in: " + cwd
```

### Change directory

```indent
os_chdir "/tmp"
say os_getcwd             #! "/tmp"
```

### Using the `path` module

```indent
get Join from path
get Dirname from path
get Basename from path

var full string = Join "/home/user" "projects" "main.ind"
say full                  #! "/home/user/projects/main.ind"

say Dirname full          #! "/home/user/projects"
say Basename full         #! "main.ind"
```

---

## Example: File Backup Script

```indent
fun backup_file path
    if not os_exists path
        flag "File not found: " + path
    
    var content string = file_read_text path
    var backup_path string = path + ".bak"
    file_write_text backup_path content
    say "Backed up to " + backup_path

do:
    backup_file "/tmp/important.txt"
catch as err:
    say "Backup failed: " + err
```

## Example: Log File Writer

```indent
fun log_message
    level
    message
    var timestamp float = time_now
    var log_line string = timestamp + " [" + level + "] " + message + "\n"
    file_append_text "/tmp/app.log" log_line

log_message "INFO" "Application started"
log_message "WARN" "Low disk space"
log_message "ERROR" "Connection lost"
```

## Example: Configuration Loader

```indent
fun load_config path
    if not os_exists path
        give {}
    
    var raw string = file_read_text path
    var config dynamic = json_loads raw
    give config

var config dynamic = load_config "/tmp/config.json"
say default dict_get config "theme" "dark"
```

---

## System Info & Process Control

```indent
# Indent version and runtime info
say sys_version()       #! "Indent 2.7.1"
say sys_platform()      #! "linux"
say sys_arch()          #! "x86_64"
say sys_executable()    #! "/home/user/.local/bin/indent"

# Command-line arguments
say sys_argv()          #! ["script.ind", "--verbose"]

# Exit with a status code
process_exit 0           # success
process_exit 1           # failure

# File hashing
var hash string = file_sha256 "/path/to/file"
say hash                #! "a1b2c3..."
```

## Practice

1. Write a script that lists all files in a directory and displays their sizes.
2. Create a simple key-value store that saves and loads from a JSON file.
3. Write a `touch` function that creates an empty file if it doesn't exist.
4. Build a script that reads environment variables for configuration, with sensible defaults for every value.

---

## Next Lesson

➡️ [Lesson 8: Advanced Functions](08-advanced-functions.md)
