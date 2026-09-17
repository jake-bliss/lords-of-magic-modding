// The map editor's whole client. No build step, no framework: this file is served verbatim out of
// the binary by `--serve`.
//
// Compositing happens here rather than on the server so that a paint redraws only the cells the
// server said changed, with no image round trip.

"use strict";

const state = {
  open: false,
  width: 0,
  height: 0,
  tiles: [],          // packed y * width + x, matching the map file's own cell order
  columns: 0,         // atlas geometry
  tileWidth: 0,
  tileHeight: 0,
  terrain: null,      // selected terrain id
  atlas: null,        // HTMLImageElement
  drag: null,
  undoAvailable: false,
};

const mapCanvas = document.getElementById("map");
const overlay = document.getElementById("overlay");
const logPanel = document.getElementById("log");

function log(text, kind) {
  for (const line of String(text).split("\n")) {
    const row = document.createElement("div");
    row.className = kind || "";
    row.textContent = line;
    logPanel.appendChild(row);
  }
  logPanel.scrollTop = logPanel.scrollHeight;
}

// Notes and refusals are the tool telling the truth about what it could and could not reproduce.
// They go in the log as readable text and stay there.
function logNotes(notes) {
  (notes || []).forEach((note) => log(note, "note"));
}

function zoom() {
  return Number(document.getElementById("zoom").value);
}

async function api(path, options) {
  const response = await fetch(path, options);
  const text = await response.text();
  try {
    return JSON.parse(text);
  } catch (error) {
    return { ok: false, refusal: `${response.status}: ${text}` };
  }
}

function form(fields) {
  const body = new URLSearchParams(fields).toString();
  return {
    method: "POST",
    headers: { "content-type": "application/x-www-form-urlencoded" },
    body,
  };
}

function drawCell(context, index) {
  const z = zoom();
  const tile = state.tiles[index];
  const x = index % state.width;
  const y = Math.floor(index / state.width);
  const sx = (tile % state.columns) * state.tileWidth;
  const sy = Math.floor(tile / state.columns) * state.tileHeight;
  context.drawImage(
    state.atlas,
    sx, sy, state.tileWidth, state.tileHeight,
    x * z, y * z, z, z,
  );
}

function redraw() {
  const z = zoom();
  mapCanvas.width = state.width * z;
  mapCanvas.height = state.height * z;
  overlay.width = mapCanvas.width;
  overlay.height = mapCanvas.height;
  const context = mapCanvas.getContext("2d");
  context.imageSmoothingEnabled = false;
  for (let index = 0; index < state.tiles.length; index += 1) {
    drawCell(context, index);
  }
}

function drawSelection(rect) {
  const z = zoom();
  const context = overlay.getContext("2d");
  context.clearRect(0, 0, overlay.width, overlay.height);
  if (!rect) {
    return;
  }
  const [x0, y0, x1, y1] = rect;
  context.strokeStyle = "#ffd479";
  context.lineWidth = 2;
  context.strokeRect(x0 * z + 1, y0 * z + 1, (x1 - x0 + 1) * z - 2, (y1 - y0 + 1) * z - 2);
}

function cellAt(event) {
  const bounds = overlay.getBoundingClientRect();
  const z = zoom();
  const x = Math.floor((event.clientX - bounds.left) / z);
  const y = Math.floor((event.clientY - bounds.top) / z);
  return [
    Math.max(0, Math.min(state.width - 1, x)),
    Math.max(0, Math.min(state.height - 1, y)),
  ];
}

function normalise(a, b) {
  return [
    Math.min(a[0], b[0]), Math.min(a[1], b[1]),
    Math.max(a[0], b[0]), Math.max(a[1], b[1]),
  ];
}

// The terrain ids come from the resolved tileset, never from a built-in list: ids are
// tileset-local and combat tilesets reach far past the world map's eleven.
function buildPalette(terrains) {
  const palette = document.getElementById("palette");
  palette.textContent = "";
  state.terrain = null;
  terrains.forEach((terrain) => {
    const swatch = document.createElement("div");
    swatch.className = terrain.paintable ? "swatch" : "swatch unpaintable";
    swatch.title = terrain.paintable
      ? `terrain ${terrain.id}: ${terrain.tiles} tiles`
      : `terrain ${terrain.id} is declared but drawn by no tile, so it cannot be painted`;
    const chip = document.createElement("canvas");
    chip.width = 20;
    chip.height = 20;
    if (terrain.paintable && state.atlas) {
      const sx = (terrain.swatch % state.columns) * state.tileWidth;
      const sy = Math.floor(terrain.swatch / state.columns) * state.tileHeight;
      chip.getContext("2d").drawImage(
        state.atlas, sx, sy, state.tileWidth, state.tileHeight, 0, 0, 20, 20,
      );
    }
    swatch.appendChild(chip);
    const label = document.createElement("span");
    label.textContent = `${terrain.id} ${terrain.description}`;
    swatch.appendChild(label);
    if (terrain.paintable) {
      swatch.addEventListener("click", () => {
        state.terrain = terrain.id;
        palette.querySelectorAll(".swatch").forEach((other) => other.classList.remove("selected"));
        swatch.classList.add("selected");
      });
    }
    palette.appendChild(swatch);
  });
}

function loadAtlas() {
  return new Promise((resolve, reject) => {
    const image = new Image();
    image.onload = () => resolve(image);
    image.onerror = () => reject(new Error("the atlas image did not load"));
    image.src = `/api/atlas.png?t=${Date.now()}`;
  });
}

async function openMap(path) {
  const result = await api(`/api/open?path=${encodeURIComponent(path)}`);
  logNotes(result.notes);
  if (!result.ok) {
    log(result.refusal, "refusal");
    document.getElementById("map-summary").textContent = "No map open.";
    state.open = false;
    return;
  }
  state.width = result.width;
  state.height = result.height;
  state.tiles = result.tiles;
  state.columns = result.tileset.columns;
  state.tileWidth = result.tileset.tileWidth;
  state.tileHeight = result.tileset.tileHeight;
  state.undoAvailable = false;
  document.getElementById("undo").disabled = true;
  state.atlas = await loadAtlas();
  state.open = true;
  buildPalette(result.terrains);
  redraw();
  drawSelection(null);
  document.getElementById("map-summary").textContent =
    `${result.path}\n${result.width}x${result.height} ${result.class}`
    + `\ntileset ${result.tileset.member} (${result.tileset.provenance})`;
  log(`opened ${result.path}`, "ok");
}

async function paint(rect) {
  if (state.terrain === null) {
    log("pick a terrain from the palette first", "refusal");
    return;
  }
  const fields = {
    x0: rect[0], y0: rect[1], x1: rect[2], y1: rect[3], terrain: state.terrain,
  };
  const seed = document.getElementById("seed").value.trim();
  if (seed !== "") {
    fields.seed = seed;
  }
  const result = await api("/api/paint", form(fields));
  logNotes(result.notes);
  if (!result.ok) {
    log(result.refusal, "refusal");
    return;
  }
  const context = mapCanvas.getContext("2d");
  result.cells.forEach((cell) => {
    state.tiles[cell.i] = cell.tile;
    drawCell(context, cell.i);
  });
  state.undoAvailable = true;
  document.getElementById("undo").disabled = false;
  log(result.summary, "ok");
}

async function undo() {
  const result = await api("/api/undo", form({}));
  logNotes(result.notes);
  if (!result.ok) {
    log(result.refusal, "refusal");
    return;
  }
  state.tiles = result.tiles;
  redraw();
  state.undoAvailable = false;
  document.getElementById("undo").disabled = true;
  log("undid the last paint", "ok");
}

document.getElementById("open-form").addEventListener("submit", (event) => {
  event.preventDefault();
  openMap(document.getElementById("open-path").value.trim());
});

document.getElementById("save-form").addEventListener("submit", async (event) => {
  event.preventDefault();
  const path = document.getElementById("save-path").value.trim();
  const result = await api("/api/save", form({ path }));
  logNotes(result.notes);
  log(result.ok ? `wrote ${result.path} (${result.bytes} bytes)` : result.refusal,
    result.ok ? "ok" : "refusal");
});

document.getElementById("undo").addEventListener("click", undo);
document.getElementById("zoom").addEventListener("input", () => {
  if (state.open) {
    redraw();
    drawSelection(null);
  }
});

overlay.addEventListener("mousedown", (event) => {
  if (!state.open) {
    return;
  }
  state.drag = cellAt(event);
  drawSelection(normalise(state.drag, state.drag));
});

overlay.addEventListener("mousemove", (event) => {
  if (state.drag) {
    drawSelection(normalise(state.drag, cellAt(event)));
  }
});

window.addEventListener("mouseup", (event) => {
  if (!state.drag) {
    return;
  }
  const rect = normalise(state.drag, cellAt(event));
  state.drag = null;
  drawSelection(null);
  paint(rect);
});
