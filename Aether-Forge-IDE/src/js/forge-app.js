// Indent Forge IDE — Professional Edition

const Forge = {
  tabs: [], activeTabId: null, folderPath: null,
  config: { model:'scrible-chat', inlineModel:'scrible-inline', endpoint:'http://localhost:11434', temperature:0.2, maxTokens:512 },
  isWaiting: false,
};

async function invoke(cmd, args) {
  args = args || {};
  try {
    if (window.__TAURI_INTERNALS__) return await window.__TAURI_INTERNALS__.invoke(cmd, args);
    if (window.__TAURI__) return await window.__TAURI__.invoke(cmd, args);
    console.warn('[Forge] Not in Tauri — using mock IPC for:', cmd);
    return { success:true, content:'', entries:[], error:null };
  } catch(e) { return { success:false, error:String(e) }; }
}

function esc(s) { const d=document.createElement('div'); d.textContent=s||''; return d.innerHTML; }

// ── Tabs ──────────────────────────────────────────────────
Forge.newTab = function(name, content, path) {
  name = name||'Untitled.ind'; content = content||'';
  const id = 'tab-'+Date.now();
  this.tabs.push({ id, name, path:path||null, content, isDirty:false });
  this.activeTabId = id; this.renderTabs(); this.showEditor();
};
Forge.showEditor = function() {
  const tab = this.tabs.find(t=>t.id===this.activeTabId);
  const ws = document.getElementById('welcome-screen'), ta = document.getElementById('editor-textarea');
  if (!tab) { if(ws)ws.style.display='flex'; if(ta)ta.style.display='none'; return; }
  if(ws) ws.style.display='none';
  if(ta) { ta.style.display='block'; ta.value = tab.content||''; ta.focus(); }
  this._updateStatus();
};
Forge.renderTabs = function() {
  const c=document.getElementById('tabs-container'); if(!c) return;
  c.innerHTML = this.tabs.map(t=>
    '<div class="vsc-tab'+(t.id===this.activeTabId?' active':'')+'" data-id="'+t.id+'">'+
    esc(t.name)+(t.isDirty?' <span style="color:var(--text3)">●</span>':'')+'</div>').join('');
  c.querySelectorAll('.vsc-tab').forEach(el=>el.addEventListener('click',()=>{
    Forge.activeTabId=el.dataset.id; Forge.showEditor(); Forge.renderTabs();
  }));
};
Forge._updateStatus = function() {
  const tab=this.tabs.find(t=>t.id===this.activeTabId);
  document.getElementById('status-file').textContent=tab?tab.name+(tab.isDirty?' ●':''):'Untitled.ind';
};
Forge.save = async function() {
  const tab=this.tabs.find(t=>t.id===this.activeTabId); if(!tab) return;
  const ta=document.getElementById('editor-textarea'); tab.content=ta?ta.value:''; tab.isDirty=false;
  if(tab.path) { await invoke('write_file',{path:tab.path,content:tab.content}); }
  this.renderTabs(); this._updateStatus();
  return tab;
};

// ── Run file ──────────────────────────────────────────────
Forge.runFile = async function(debug) {
  let tab = this.tabs.find(t=>t.id===this.activeTabId);
  if(!tab) return;
  const ta=document.getElementById('editor-textarea');
  tab.content=ta?ta.value:''; tab.isDirty=false;
  if(!tab.path) {
    tab.path = (this.folderPath||'/tmp') + '/' + (tab.name||'untitled.ind');
    await invoke('write_file',{path:tab.path,content:tab.content});
  } else {
    await invoke('write_file',{path:tab.path,content:tab.content});
  }
  this.renderTabs(); this._updateStatus();
  clearOutput(); logOutput('Launched in terminal: '+tab.name+'\n');
  switchPanel('output');
  await invoke('run_indent',{path:tab.path,debug:!!debug});
};

Forge.toggleScrible = function() {
  document.getElementById('sidebar-right').classList.toggle('collapsed');
};

// ── File Tree ─────────────────────────────────────────────
// Seti icon mapping: file extension → [unicode char, color]
const SETI_ICONS = {
  // Indent (custom SVGs, not in Seti font)
  ath: ['svg','assets/indent.svg'],
  glo: ['svg','assets/indent-glo.svg'],
  // Seti font characters
  py:   ['\uE07B','#519aba'],    // Python
  js:   ['\uE051','#cbcb41'],    // JavaScript
  ts:   ['\uE06D','#519aba'],    // TypeScript
  jsx:  ['\uE052','#519aba'],    // React JSX
  tsx:  ['\uE06E','#519aba'],    // React TSX
  json: ['\uE055','#cbcb41'],    // JSON
  md:   ['\uE060','#519aba'],    // Markdown
  rs:   ['\uE082','#6d8086'],    // Rust
  sh:   ['\uE089','#8dc149'],    // Shell
  bash: ['\uE089','#8dc149'],
  zsh:  ['\uE089','#8dc149'],
  ps1:  ['\uE089','#8dc149'],
  html: ['\uE048','#519aba'],    // HTML
  htm:  ['\uE048','#519aba'],
  css:  ['\uE01D','#519aba'],    // CSS
  toml: ['\uE019','#6d8086'],    // Config
  cfg:  ['\uE019','#6d8086'],
  ini:  ['\uE019','#6d8086'],
  conf: ['\uE019','#6d8086'],
  lock: ['\uE05D','#8dc149'],    // Lock
  gitignore: ['\uE034','#41535b'], // Git
  git:      ['\uE034','#41535b'],
  svg: ['\uE04C','#a074c4'],    // Image/SVG
  png: ['\uE04C','#a074c4'],
  jpg: ['\uE04C','#a074c4'],
  jpeg:['\uE04C','#a074c4'],
  gif: ['\uE04C','#a074c4'],
  webp:['\uE04C','#a074c4'],
  ico: ['\uE04C','#a074c4'],
  xml: ['\uE01D','#519aba'],    // XML
};
const SETI_DEFAULT = ['\uE023','#d4d7d6'];  // default file

// ═══════════════════════════════════════════════════════════
// Scrible System Prompt — the single source of truth for Indent syntax
// ═══════════════════════════════════════════════════════════
const SCRIBLE_SYSTEM_PROMPT = `You are Scrible, an AI pair programmer embedded in Indent Forge IDE by Xytro Labs. You write ONLY valid Indent code. You NEVER use Python, JavaScript, C, or any other language syntax.

=== INDENT SYNTAX — ABSOLUTE RULES ===
Comments:   #! comment text        (NEVER use # alone, //, /* */, --, or <!-- -->)
Functions:  fun name param1 param2   (NO "def", NO "function", NO "fn", NO colons, NO braces, NO arrows)
            indented body
Return:     give value               (NEVER "return", NEVER "yield")
Print:      say expression           (NEVER "print", "echo", "console.log", "puts")
Vars:       var name type = value    (types: string int float boolean dynamic empty list dict)
Reassign:   name is value            (NOT "=", NOT ":=", NOT "let")
If:         if condition             (NO "elif", NO "else:", NO "endif", NEVER "else if")
                body
            otherwise
                body
Loops:      repeat n                 (NEVER "for", "while", "do")
            repeat item in list
Import:     get Module               (NEVER "import", "require", "include")
Input:      ask("prompt")            (returns string; use ask("int","prompt") for int, etc.)
Types:      string int float boolean dynamic empty list dict

=== WRONG → RIGHT (study these!) ===
def foo(x):          → fun foo x
def foo(x):\n  ...   → fun foo x\n    ...
function bar(){}     → fun bar\n    ...
return x + 1         → give x + 1
print("Hello")       → say "Hello"
echo "hi"            → say "hi"
x = 42               → x is 42
var x = 42           → var x int = 42
for i in range(5):   → repeat 5
for item in list:    → repeat item in list
if x > 0:\n  ...     → if x > 0\n    ...
elif x < 0:          → or x < 0  (use "or" not "elif")
else:                → otherwise
import math          → get math
type(x) == 'int'     → (type checking not used in Indent; trust the type system)
isinstance(x, int)   → (NOT valid Indent)

=== HOW TO WRITE A FUNCTION ===
When asked \"write a function that does X\", produce:
1. A #! comment explaining what it does
2. The fun definition with parameters
3. Indented body
4. give for the return value
5. Optionally, a say call to show usage

Example request: "write a function that doubles a number"
Correct response:
\`\`\`indent
#! Double a number
fun double n
    give n * 2

var result int = double(5)
say "5 doubled is " + result
\`\`\`

=== COMPLETE EXAMPLES ===

List summing:
\`\`\`indent
#! Sum all numbers in a list
fun sum_list numbers
    var total int = 0
    repeat n in numbers
        total is total + n
    give total

var nums list = [1, 2, 3, 4, 5]
say sum_list(nums)
\`\`\`

FizzBuzz:
\`\`\`indent
#! FizzBuzz from 1 to n
fun fizzbuzz n
    var i int = 1
    repeat n
        if i % 15 == 0
            say "FizzBuzz"
        or i % 3 == 0
            say "Fizz"
        or i % 5 == 0
            say "Buzz"
        otherwise
            say i
        i is i + 1

fizzbuzz(20)
\`\`\`

Dictionary usage:
\`\`\`indent
var person dict = {"name": "Ada", "age": 28}
say person["name"]
say person.name        # dot notation works too
person["age"] is 29
var keys list = keys(person)
var has_name boolean = has_key(person, "name")
\`\`\`

=== FILE WRITING ===
If the user asks you to write code to a specific file, start your code block with a file marker:
\`\`\`indent
#! FILE: path/to/file.ind
(code here)
\`\`\`
The IDE will automatically save it. Use the exact path the user mentions.

=== RULES ===
- Only output valid Indent code. Never Python, JS, C, etc.
- Wrap code in \`\`\`indent blocks.
- Keep explanations brief — show the code.
- Never use "def", "return", "print", "elif", "else:", "for", "while", "import".
- When in doubt, use #! comments not # comments.
- Reassignment uses "is" not "=".
- Parenthesized calls work everywhere: double(5), say("hi"), if check(x).`;

function fileIcon(name, isDir) {
  if (isDir) return '<span class="si-icon si-folder"></span>';
  const ext = (name||'').split('.').pop().toLowerCase();
  const m = SETI_ICONS[ext];
  if (!m) return `<span class="si-icon" style="color:${SETI_DEFAULT[1]}">${SETI_DEFAULT[0]}</span>`;
  if (m[0]==='svg') return `<img src="${m[1]}" width="16" height="16" style="vertical-align:middle;flex-shrink:0">`;
  return `<span class="si-icon" style="color:${m[1]}">${m[0]}</span>`;
}

async function loadDir(dirPath, parentEl) {
  Forge.folderPath=dirPath;
  const empty=document.getElementById('files-empty'), tree=document.getElementById('files-tree');
  const r=await invoke('list_dir',{path:dirPath}); if(!r||!r.success||!r.entries) return;
  if(empty)empty.style.display='none'; if(tree)tree.style.display='block';
  const target = parentEl || tree;
  if(target) target.innerHTML = r.entries.map(e=>{
    const cls = e.name.startsWith('.')?' ft-dim':'';
    const icon = fileIcon(e.name, e.is_directory);
    return '<div class="ft-row'+cls+'" data-path="'+esc(e.path)+'" data-dir="'+e.is_directory+'">'+
      (e.is_directory?'<span class="ft-chevron">&#9656;</span>':'<span style="width:14px;flex-shrink:0"></span>')+
      '<span class="ft-icon">'+icon+'</span>'+
      '<span class="ft-name">'+esc(e.name)+'</span></div>'+
      (e.is_directory?'<div class="ft-children" data-parent="'+esc(e.path)+'"></div>':'');
  }).join('');
  (parentEl||tree).querySelectorAll('.ft-row').forEach(el=>el.addEventListener('click',async function(){
    (parentEl||tree).querySelectorAll('.ft-row.selected').forEach(x=>x.classList.remove('selected'));
    el.classList.add('selected');
    if(el.dataset.dir==='true'){
      const children = el.nextElementSibling;
      if(children&&children.classList.contains('ft-children')){
        if(children.classList.contains('open')){
          children.classList.remove('open'); children.innerHTML=''; el.querySelector('.ft-chevron').innerHTML='&#9656;';
        } else {
          children.classList.add('open'); el.querySelector('.ft-chevron').innerHTML='&#9662;';
          await loadDir(el.dataset.path, children);
        }
      }
    } else {
      const rr=await invoke('read_file',{path:el.dataset.path});
      if(rr&&rr.success) Forge.newTab(el.dataset.path.split('/').pop(),rr.content,el.dataset.path);
    }
  }));
}

// ── Open Folder ──────────────────────────────────────────
function openFolderDialog() {
  const absPath = prompt('Open folder (enter absolute path):', Forge.folderPath||'/run/media/raf/Z/Indent.ind');
  if(absPath && absPath.trim()) loadDir(absPath.trim());
}

// ── Scrible Chat ──────────────────────────────────────────
function getFullFileContent() {
  const ta = document.getElementById('editor-textarea');
  if (!ta || ta.style.display === 'none') return '';
  return ta.value;
}

function getSelectedCode() {
  const ta = document.getElementById('editor-textarea');
  if (!ta || ta.style.display === 'none') return '';
  const start = ta.selectionStart, end = ta.selectionEnd;
  if (start !== end) return ta.value.substring(start, end);
  return '';
}

function updateSmartPrompts() {
  const container = document.getElementById('scrible-prompts');
  if (!container) return;
  const hasCode = !!getFullFileContent().trim();
  const hasSelection = !!getSelectedCode().trim();
  let prompts;
  if (hasSelection) {
    prompts = [
      { label:'Explain selection', prompt:'Explain what this Indent code does:\n```indent\n'+getSelectedCode()+'\n```' },
      { label:'Fix selection', prompt:'Find and fix bugs in this Indent code. Remember: NO Python syntax, NO braces, NO "def", NO "return", NO "print". Use #! comments, fun/var/give/say:\n```indent\n'+getSelectedCode()+'\n```' },
    ];
  } else if (hasCode) {
    prompts = [
      { label:'Explain this file', prompt:'Explain what this Indent code does. Note: #! is for comments, # alone is a hex color.' },
      { label:'Review for bugs', prompt:'Review this Indent code for bugs. CRITICAL: NO Python/C syntax. Indent uses: fun name (then indent body), var name type = value, give (not return), say (not print), #! comments (not #).' },
    ];
  } else {
    prompts = [
      { label:'Explain Indent syntax', prompt:'What are the key syntax rules of Indent? I need to know about comments, functions, variables, and control flow.' },
      { label:'Debug help', prompt:'Help me debug this issue. Remember: Indent uses #! comments, fun/var/give/say keywords, indentation-based blocks, and parenthesized function calls.' },
    ];
  }
  container.innerHTML = prompts.map(p=>
    '<button class="prompt-chip" data-prompt="'+esc(p.prompt)+'">'+esc(p.label)+'</button>'
  ).join('');
  container.querySelectorAll('.prompt-chip').forEach(ch=>{
    ch.addEventListener('click',()=>{
      document.getElementById('scrible-input').value = ch.dataset.prompt;
      scribleSend(ch.dataset.prompt);
    });
  });
}

async function scribleSend(text) {
  if(!text||!text.trim()||Forge.isWaiting) return;
  document.getElementById('scrible-input').value = '';
  addBubble('user',text); Forge.isWaiting=true; showTyping(true);
  const fileContent = getFullFileContent();
  const tab = Forge.tabs.find(t=>t.id===Forge.activeTabId);
  const fileInfo = tab&&tab.name ? ' (file: '+tab.name+')' : '';
  const projCtx = tab&&tab.path ? '\nCurrent file path: '+tab.path : '';
  const ctxBlock = fileContent ? '\n\nThe user has this file open in the editor:\n```indent\n'+fileContent+'\n```' : '';
  try {
    const r = await invoke('scrible_chat', {query:{
      endpoint:Forge.config.endpoint, model:Forge.config.model,
      messages:[
        {role:'system', content:SCRIBLE_SYSTEM_PROMPT},
        {role:'user', content:text+fileInfo+projCtx+ctxBlock}
      ], temperature:Forge.config.temperature, max_tokens:Forge.config.maxTokens
    }});
    const response = (r&&r.success&&r.response)?r.response:(r&&r.error)?'**Error:** '+esc(r.error):'_(No response. Is Ollama running?)_';
    addBubble('bot', response);
    // Check for file-write directives and auto-apply
    autoApplyWrites(response);
  } catch(e){ addBubble('bot','**Error:** '+esc(String(e))); }
  Forge.isWaiting=false; showTyping(false);
  updateSmartPrompts();
}

// ── Auto-apply file writes from Scrible responses ────────
async function autoApplyWrites(response) {
  // Look for #! FILE: path.ind at the start of code blocks
  const re = /```(?:indent)?\s*\n#!\s*FILE:\s*(\S+)\s*\n([\s\S]*?)```/g;
  let match, wrote = [];
  while ((match = re.exec(response)) !== null) {
    const filePath = match[1].trim();
    const code = match[2].trim();
    const fullPath = (Forge.folderPath||'') + '/' + filePath;
    try {
      await invoke('write_file', {path: fullPath, content: code});
      wrote.push(filePath);
    } catch(e) {
      console.error('Auto-write failed for', filePath, e);
    }
  }
  if (wrote.length > 0) {
    logOutput('Scrible wrote: ' + wrote.join(', ') + '\n');
    // Refresh file tree if visible
    if (Forge.folderPath) {
      const tree = document.getElementById('files-tree');
      if (tree && tree.style.display !== 'none') {
        // Re-expand the tree — simple approach: reload root
        loadDir(Forge.folderPath);
      }
    }
    // If one of the files is currently open, update it
    const tab = Forge.tabs.find(t => t.id === Forge.activeTabId);
    if (tab && wrote.some(f => tab.path && tab.path.endsWith(f))) {
      const r = await invoke('read_file', {path: tab.path});
      if (r && r.success && r.content !== undefined) {
        tab.content = r.content;
        tab.isDirty = false;
        const ta = document.getElementById('editor-textarea');
        if (ta) ta.value = r.content;
        Forge.renderTabs();
        Forge._updateStatus();
      }
    }
  }
}

function addBubble(role,text) {
  const c=document.getElementById('scrible-messages');
  const welcome=c?c.querySelector('.scrible-welcome'):null; if(welcome)welcome.remove(); if(!c)return;
  const b=document.createElement('div'); b.className='scrible-msg '+role;
  const label=document.createElement('div'); label.className='msg-role';
  label.textContent=role==='user'?'You':'Scrible'; b.appendChild(label);
  const content=document.createElement('div');
  let h=esc(text);
  h=h.replace(/```(\w*)\n?([\s\S]*?)```/g,(_,lang,code)=>'<pre><code>'+esc(code.trim())+'</code></pre>');
  h=h.replace(/`([^`]+)`/g,'<code>$1</code>');
  h=h.replace(/\*\*([^*]+)\*\*/g,'<strong>$1</strong>');
  h=h.replace(/\n/g,'<br>'); content.innerHTML=h; b.appendChild(content);
  // Code actions
  if(role==='bot'){
    const codeBlock=b.querySelector('pre code');
    if(codeBlock){
      const actions=document.createElement('div'); actions.className='scrible-msg-actions';
      const insertBtn=document.createElement('button'); insertBtn.textContent='Insert at cursor';
      insertBtn.addEventListener('click',()=>{
        const ta=document.getElementById('editor-textarea');
        if(ta){
          const code=codeBlock.textContent||'';
          const s=ta.selectionStart, e=ta.selectionEnd;
          ta.setRangeText('\n'+code+'\n', s, e, 'end');
          ta.focus();
          const tab=Forge.tabs.find(t=>t.id===Forge.activeTabId);
          if(tab){tab.isDirty=true;tab.content=ta.value;Forge.renderTabs();Forge._updateStatus();}
        }
      });
      const copyBtn=document.createElement('button'); copyBtn.textContent='Copy';
      copyBtn.addEventListener('click',()=>{
        navigator.clipboard.writeText(codeBlock.textContent||'').then(()=>{copyBtn.textContent='Copied!';setTimeout(()=>{copyBtn.textContent='Copy';},1500);});
      });
      const writeBtn=document.createElement('button'); writeBtn.textContent='Write to file';
      writeBtn.addEventListener('click',async()=>{
        const code=codeBlock.textContent||'';
        const tab=Forge.tabs.find(t=>t.id===Forge.activeTabId);
        const defaultPath = tab&&tab.path ? tab.path : (Forge.folderPath||'/tmp')+'/new.ind';
        const filePath = prompt('Write code to file:', defaultPath);
        if (filePath) {
          await invoke('write_file',{path:filePath,content:code});
          logOutput('Written to: '+filePath+'\n');
          if (Forge.folderPath) loadDir(Forge.folderPath);
        }
      });
      actions.appendChild(insertBtn); actions.appendChild(writeBtn); actions.appendChild(copyBtn); b.appendChild(actions);
    }
  }
  c.appendChild(b); c.scrollTop=c.scrollHeight;
}

function clearScribleChat(){const c=document.getElementById('scrible-messages');if(!c)return;c.innerHTML='<div class="scrible-welcome"><svg width="32" height="32" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.2"><circle cx="12" cy="12" r="9"/><path d="M9 10h.01M15 10h.01M9.5 15c.8.7 1.9 1 3 1s2.2-.3 3-1"/></svg><h3>Scrible</h3><p>Your AI pair programmer — ask me anything about your code</p><p class="scrible-note">Powered by Llama 3.1 8B · Preinstalled &amp; ready</p></div>';updateSmartPrompts();}
function showTyping(s){const e=document.getElementById('scrible-typing');if(e)e.style.display=s?'flex':'none';}

// ── Output ────────────────────────────────────────────────
function logOutput(text){const t=document.getElementById('output-lines');if(!t)return;t.querySelector('.output-placeholder')?.remove();const s=document.createElement('span');s.textContent=text;t.appendChild(s);t.scrollTop=t.scrollHeight;}
function clearOutput(){const t=document.getElementById('output-lines');if(t)t.innerHTML='';}
function switchPanel(name){document.querySelectorAll('#panel-tabs .panel-tab').forEach(b=>b.classList.remove('active'));document.querySelectorAll('.panel-content').forEach(p=>p.classList.remove('active'));const tab=document.querySelector('#panel-tabs .panel-tab[data-panel="'+name+'"]'),content=document.getElementById('panel-'+name);if(tab)tab.classList.add('active');if(content)content.classList.add('active');}

// ── Init ──────────────────────────────────────────────────
document.addEventListener('DOMContentLoaded',()=>{
  document.querySelectorAll('.act-btn[data-panel]').forEach(b=>b.addEventListener('click',()=>{
    const p=b.dataset.panel; document.querySelectorAll('.act-btn').forEach(x=>x.classList.remove('active')); b.classList.add('active');
    if(p==='scrible'){document.getElementById('sidebar-right').classList.toggle('collapsed');}
    else if(p==='explorer'){document.getElementById('sidebar-left').classList.remove('collapsed');}
  }));
  document.getElementById('btn-collapse-left')?.addEventListener('click',()=>document.getElementById('sidebar-left').classList.add('collapsed'));
  document.getElementById('btn-collapse-right')?.addEventListener('click',()=>document.getElementById('sidebar-right').classList.add('collapsed'));

  // Title bar buttons
  document.getElementById('btn-new-file')?.addEventListener('click',()=>Forge.newTab());
  document.getElementById('btn-save-file')?.addEventListener('click',()=>Forge.save());
  document.getElementById('btn-run')?.addEventListener('click',()=>Forge.runFile(false));
  document.getElementById('btn-debug')?.addEventListener('click',()=>Forge.runFile(true));
  document.getElementById('btn-stop')?.addEventListener('click',()=>{invoke('stop_execution');logOutput('Stopped.\n');});
  document.getElementById('btn-new-tab')?.addEventListener('click',()=>Forge.newTab());
  document.getElementById('btn-clear-output')?.addEventListener('click',clearOutput);

  // Welcome + Open Folder — uses native folder picker
  document.getElementById('welcome-new')?.addEventListener('click',()=>Forge.newTab());
  document.getElementById('welcome-folder')?.addEventListener('click',openFolderDialog);
  document.getElementById('btn-open-folder')?.addEventListener('click',openFolderDialog);

  // Editor
  const ta=document.getElementById('editor-textarea');
  ta?.addEventListener('input',()=>{const tab=Forge.tabs.find(t=>t.id===Forge.activeTabId);if(tab){tab.isDirty=true;tab.content=ta.value;Forge.renderTabs();Forge._updateStatus();}});
  ta?.addEventListener('keydown',e=>{if(e.key==='Tab'){e.preventDefault();const s=ta.selectionStart;ta.value=ta.value.slice(0,s)+'    '+ta.value.slice(ta.selectionEnd);ta.selectionStart=ta.selectionEnd=s+4;}});
  const uc=()=>{const p=document.getElementById('status-cursor');if(p&&ta){const lines=ta.value.substr(0,ta.selectionStart).split('\n');p.textContent='Ln '+lines.length+', Col '+(lines[lines.length-1].length+1);}};
  ta?.addEventListener('click',uc); ta?.addEventListener('keyup',uc);

  // Bottom panel
  document.querySelectorAll('#panel-tabs .panel-tab').forEach(b=>b.addEventListener('click',()=>switchPanel(b.dataset.panel)));
  document.getElementById('btn-close-panel')?.addEventListener('click',()=>{document.getElementById('bottom-panel').style.display='none';});

  // Scrible
  const si=document.getElementById('scrible-input');
  document.getElementById('scrible-send')?.addEventListener('click',()=>scribleSend(si?.value));
  si?.addEventListener('keydown',e=>{if(e.key==='Enter'&&!e.shiftKey){e.preventDefault();scribleSend(si.value);}});
  document.getElementById('btn-clear-chat')?.addEventListener('click',clearScribleChat);
  ta?.addEventListener('input',updateSmartPrompts);
  ta?.addEventListener('mouseup',updateSmartPrompts);
  document.getElementById('scrible-model')?.addEventListener('change',function(){Forge.config.model=this.value;document.getElementById('status-model').textContent=this.value;try{localStorage.setItem('forge-config',JSON.stringify(Forge.config));}catch(e){}});
  updateSmartPrompts();

  // Config
  try{const s=JSON.parse(localStorage.getItem('forge-config')||'{}');Object.assign(Forge.config,s);const sm=document.getElementById('scrible-model');if(sm)sm.value=Forge.config.model;document.getElementById('status-model').textContent=Forge.config.model;}catch(e){}

  // Keyboard
  document.addEventListener('keydown',e=>{if((e.ctrlKey||e.metaKey)&&e.key==='s'){e.preventDefault();Forge.save();}});
  document.addEventListener('keydown',e=>{if((e.ctrlKey||e.metaKey)&&e.key==='b'){e.preventDefault();document.getElementById('sidebar-left').classList.toggle('collapsed');}});
  document.addEventListener('keydown',e=>{if((e.ctrlKey||e.metaKey)&&e.key==='j'){e.preventDefault();Forge.toggleScrible();}});
  document.getElementById('act-settings')?.addEventListener('click',()=>alert('Indent Forge v2.1\nXytro Labs\nTauri 2.0 + Rust + Scrible AI'));
  console.log('Indent Forge ready.');
  // Ensure right sidebar visible
  document.getElementById('sidebar-right')?.classList.remove('collapsed');

  // ── Integrated Terminal ──────────────────────────────────
  Forge._terminalPolling = null;
  Forge._terminalRunning = false;

  Forge.runTerminalCommand = async function(cmd) {
    const out = document.getElementById('terminal-output');
    if (out) out.textContent = '';
    this._terminalRunning = true;
    const r = await invoke('run_shell', {command: cmd});
    if (out && r.output) out.textContent += r.output;
    if (r.error) logOutput('Error: '+r.error);
    this._startPolling();
  };

  Forge._startPolling = function() {
    if (this._terminalPolling) clearInterval(this._terminalPolling);
    this._terminalPolling = setInterval(async () => {
      if (!this._terminalRunning) { clearInterval(this._terminalPolling); this._terminalPolling = null; return; }
      const r = await invoke('terminal_read', {});
      const out = document.getElementById('terminal-output');
      if (r.success && r.output && out) {
        out.textContent += r.output;
        out.scrollTop = out.scrollHeight;
      }
      if (r.error) { this._terminalRunning = false; logOutput('Terminal: '+r.error); }
    }, 200);
  };

  Forge.sendTerminalInput = async function(input) {
    if (!input) return;
    const out = document.getElementById('terminal-output');
    if (out) out.textContent += '$ ' + input + '\n';
    await invoke('terminal_write', {input: input});
    document.getElementById('terminal-input').value = '';
  };

  document.getElementById('terminal-input')?.addEventListener('keydown', async (e) => {
    if (e.key === 'Enter') {
      e.preventDefault();
      const val = e.target.value.trim();
      if (!val) return;
      if (!Forge._terminalRunning) {
        await Forge.runTerminalCommand(val);
      } else {
        await Forge.sendTerminalInput(val);
      }
    }
  });

  document.getElementById('act-terminal')?.addEventListener('click', () => {
    const panel = document.getElementById('bottom-panel');
    panel.style.display = 'flex';
    switchPanel('terminal');
    document.getElementById('terminal-input')?.focus();
  });
});
