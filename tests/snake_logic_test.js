// Headless validation of the Snake game logic (mirrors examples/snake_game.html).
// Run: node tests/snake_logic_test.js
function createGame() {
  var snake = [{x: 10, y: 10}, {x: 9, y: 10}, {x: 8, y: 10}];
  var dir = {x: 1, y: 0};
  var nextDir = {x: 1, y: 0};
  var score = 0;
  var food = {x: 15, y: 10};
  var running = true;
  var GRID = 20;
  function placeFood() {
    do {
      food = {x: Math.floor(Math.random() * GRID), y: Math.floor(Math.random() * GRID)};
    } while (snake.some(s => s.x === food.x && s.y === food.y));
  }
  function step() {
    if (!running) return 'over';
    dir = nextDir;
    var head = {x: snake[0].x + dir.x, y: snake[0].y + dir.y};
    if (head.x < 0 || head.x >= GRID || head.y < 0 || head.y >= GRID) { running = false; return 'wall'; }
    if (snake.some(s => s.x === head.x && s.y === head.y)) { running = false; return 'self'; }
    snake.unshift(head);
    if (head.x === food.x && head.y === food.y) { score += 10; placeFood(); }
    else snake.pop();
    return 'ok';
  }
  function botStep() {
    var h = snake[0];
    if (food.x > h.x && dir.x !== -1) nextDir = {x: 1, y: 0};
    else if (food.x < h.x && dir.x !== 1) nextDir = {x: -1, y: 0};
    else if (food.y > h.y && dir.y !== -1) nextDir = {x: 0, y: 1};
    else if (food.y < h.y && dir.y !== 1) nextDir = {x: 0, y: -1};
  }
  return {
    snake, dir, step, botStep, placeFood,
    getScore: () => score, isRunning: () => running,
    setFood: (f) => { food = f; },
    setSnake: (s) => { snake = s; },
    setDir: (d) => { dir = d; nextDir = d; }
  };
}

function assert(cond, msg) {
  if (cond) console.log("PASS: " + msg);
  else { console.log("FAIL: " + msg); process.exit(1); }
}

var g = createGame();
g.step();
assert(g.snake[0].x === 11 && g.snake[0].y === 10, "move right");

g.setFood({x: 12, y: 10});
var before = g.snake.length;
g.step();
assert(g.snake.length === before + 1 && g.getScore() === 10, "eat+score");

var g3 = createGame();
g3.setSnake([{x: 19, y: 5}]);
g3.setDir({x: 1, y: 0});
assert(g3.step() === 'wall' && !g3.isRunning(), "wall collision");

var g4 = createGame();
g4.setSnake([{x: 5, y: 5}, {x: 5, y: 6}, {x: 4, y: 6}, {x: 4, y: 5}]);
g4.setDir({x: -1, y: 0});
assert(g4.step() === 'self' && !g4.isRunning(), "self collision");

var g5 = createGame();
var steps = 0, scored = false;
while (steps < 200 && g5.isRunning()) {
  g5.botStep();
  g5.step();
  if (g5.getScore() > 0) scored = true;
  steps++;
}
assert(scored, "bot scores (" + g5.getScore() + " points in " + steps + " frames)");

console.log("\nALL SNAKE LOGIC TESTS PASSED");
