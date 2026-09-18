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
  // The handle `/api/open` hands out. The editor holds one map per process, so a second tab
  // opening a different map makes this one stale -- and a stale handle is refused rather than
  // silently painting into a map this canvas is not showing.
  token: "",
  path: "",
  // The directory the user chose. Open and Save both work inside it, so nobody types an absolute
  // path twice -- the first attempt at this tool failed on whitespace pasted into one.
  directory: "",
  undoDepth: 0,
  // A full path chosen through the native save dialog. When set it is sent instead of the
  // directory-and-filename pair, and it goes through exactly the same server-side guards.
  savePath: "",
};

const mapCanvas = document.getElementById("map");
const overlay = document.getElementById("overlay");
const readout = document.getElementById("cursor-cell");
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

// The button says how far back the session can go, because the depth is bounded and a user who
// believes it is unlimited will discover otherwise at the worst moment.
function setUndoDepth(depth) {
  state.undoDepth = depth ?? 0;
  const button = document.getElementById("undo");
  button.disabled = state.undoDepth === 0;
  button.textContent = state.undoDepth === 0 ? "Undo" : `Undo (${state.undoDepth})`;
}

function zoom() {
  return Number(document.getElementById("zoom").value);
}

async function api(path, options) {
  // A dead server must not look like a dead editor. `fetch` *rejects* when nothing is
  // listening, and an unhandled rejection in an async click handler logs nothing and shows
  // nothing -- so every button silently stops working and the tool looks broken. That is
  // exactly how the first human to use this lost a session: the server had been reaped, and
  // "I can't paint after undo" was the only symptom available to them.
  let response;
  try {
    response = await fetch(path, options);
  } catch (error) {
    return {
      ok: false,
      refusal:
        `cannot reach the editor server: ${error}. It is probably no longer running -- ` +
        `restart it and reload this page. Nothing was written.`,
    };
  }
  const text = await response.text();
  try {
    return JSON.parse(text);
  } catch (error) {
    return { ok: false, refusal: `${response.status}: ${text}` };
  }
}

function form(fields) {
  const body = new URLSearchParams({ ...fields, token: state.token }).toString();
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
    image.src = `/api/atlas.png?token=${encodeURIComponent(state.token)}&t=${Date.now()}`;
  });
}

async function listDirectory(directory) {
  const result = await api(`/api/list?dir=${encodeURIComponent(directory)}`);
  logNotes(result.notes);
  const picker = document.getElementById("map-file");
  if (!result.ok) {
    log(result.refusal, "refusal");
    picker.textContent = "";
    return;
  }
  state.directory = result.dir;
  picker.textContent = "";
  result.entries.forEach((entry) => {
    const option = document.createElement("option");
    option.value = entry.name;
    option.textContent = `${entry.name} (${entry.size} bytes)`;
    picker.appendChild(option);
  });
  log(`${result.entries.length} maps in ${result.dir}`, "ok");
}

async function openMap(path) {
  // POST, not GET. Opening replaces the server's whole session, and a state-mutating GET is
  // reachable from a bare `<img src>` on any page in the world.
  const result = await api("/api/open", {
    method: "POST",
    headers: { "content-type": "application/x-www-form-urlencoded" },
    body: new URLSearchParams({ path }).toString(),
  });
  logNotes(result.notes);
  if (!result.ok) {
    log(result.refusal, "refusal");
    // **A refused open does not close the map already open.** The server goes on holding it, and
    // saying "No map open." here while Save As still writes is how a typo produced a file the user
    // had been told did not exist.
    if (result.holding) {
      log(`still holding ${result.holding}; the new path was refused`, "note");
    } else {
      document.getElementById("map-summary").textContent = "No map open.";
      state.open = false;
    }
    return;
  }
  state.token = result.token;
  state.path = result.path;
  state.width = result.width;
  state.height = result.height;
  state.tiles = result.tiles;
  state.columns = result.tileset.columns;
  state.tileWidth = result.tileset.tileWidth;
  state.tileHeight = result.tileset.tileHeight;
  setUndoDepth(0);
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
  setUndoDepth(result.undoDepth);
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
  setUndoDepth(result.undoDepth);
  log("undid the last paint", "ok");
}

// The browser cannot hand the server a real filesystem path: `webkitdirectory` gives file contents
// with fake relative names, and the File System Access API gives an opaque handle and is Chrome
// only. Neither yields `/Users/...`, which is what the server has to read and write. **The server
// runs on this machine, so the server opens the dialog** and hands back the genuine path.
//
// The typed fields are not going away. This is an accelerator for them.
async function browse(endpoint, fields) {
  const result = await api(endpoint, form(fields));
  logNotes(result.notes);
  if (!result.ok) {
    log(result.refusal, "refusal");
    return null;
  }
  // Dismissing a dialog is an ordinary thing to do and says nothing worth logging.
  return result.cancelled ? null : result.path;
}

document.getElementById("browse-dir").addEventListener("click", async () => {
  const chosen = await browse("/api/pick-directory", { dir: state.directory });
  if (chosen === null) {
    return;
  }
  document.getElementById("maps-dir").value = chosen;
  await listDirectory(chosen);
});

document.getElementById("browse-save").addEventListener("click", async () => {
  if (!state.open) {
    log("no map is open in this tab, so there is nothing to save", "refusal");
    return;
  }
  const chosen = await browse("/api/pick-save", {
    dir: state.directory,
    name: document.getElementById("save-name").value.trim(),
  });
  if (chosen === null) {
    return;
  }
  state.savePath = chosen;
  document.getElementById("save-name").value = chosen;
  log(`saving to ${chosen} when you press Save As`, "note");
});

// A typed filename replaces a browsed path: whichever the user touched last is the one that counts.
document.getElementById("save-name").addEventListener("input", () => {
  state.savePath = "";
});

document.getElementById("dir-form").addEventListener("submit", (event) => {
  event.preventDefault();
  listDirectory(document.getElementById("maps-dir").value.trim());
});

document.getElementById("open-form").addEventListener("submit", (event) => {
  event.preventDefault();
  const name = document.getElementById("map-file").value;
  if (!state.directory || !name) {
    log("choose a maps directory and a file in it first", "refusal");
    return;
  }
  // The name came out of the server's own listing, so the client is not inventing a path -- it is
  // rejoining one the server already produced.
  openMap(`${state.directory.replace(/\/$/, "")}/${name}`);
});

document.getElementById("save-form").addEventListener("submit", async (event) => {
  event.preventDefault();
  // Gated on the client's own belief as well as the server's handle: if this page is not showing a
  // map, it must not write one. The two used to be able to disagree silently.
  if (!state.open) {
    log("no map is open in this tab, so there is nothing to save", "refusal");
    return;
  }
  const name = document.getElementById("save-name").value.trim();
  if (!name) {
    log("type a filename to save as", "refusal");
    return;
  }
  // A browsed path is sent whole; a typed name is sent with the directory it belongs to, and the
  // server refuses anything that is not one plain component. Both go through the same create-new
  // and same-file guards.
  const result = state.savePath
    ? await api("/api/save", form({ path: state.savePath }))
    : await api("/api/save", form({ dir: state.directory, name }));
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

// Zoom **to the cursor**, not to the origin.
//
// At zoom 32 a 128x128 map is 4096 pixels square, so zooming about the top-left corner throws
// whatever the user was looking at off the screen and makes the slider useless. Keeping the cell
// under the cursor under the cursor is what makes a wheel usable on a map this size.
//
// The arithmetic is deliberately expressed in the same terms `cellAt` uses -- the overlay's
// bounding rect and the zoom -- so the two cannot drift apart. `cell` is fractional here: rounding
// it would drift by up to half a cell per notch.
function zoomToCursor(nextZoom, clientX, clientY) {
  const slider = document.getElementById("zoom");
  const previous = zoom();
  const clamped = Math.max(Number(slider.min), Math.min(Number(slider.max), nextZoom));
  if (clamped === previous) {
    return;
  }
  const before = overlay.getBoundingClientRect();
  const cellX = (clientX - before.left) / previous;
  const cellY = (clientY - before.top) / previous;

  slider.value = String(clamped);
  redraw();
  drawSelection(null);

  // After the resize the canvas still starts wherever the scroll left it. Scrolling by the
  // difference between where the cell now is and where the cursor is puts it back.
  const pane = document.getElementById("centre");
  const after = overlay.getBoundingClientRect();
  pane.scrollLeft += after.left + cellX * clamped - clientX;
  pane.scrollTop += after.top + cellY * clamped - clientY;
}

overlay.addEventListener("wheel", (event) => {
  if (!state.open) {
    return;
  }
  // A trackpad pinch arrives as a wheel event with `ctrlKey` set; so does ctrl-scroll. A plain
  // wheel is left alone so the pane scrolls, which is what a user expects on a map larger than
  // the window.
  if (!event.ctrlKey) {
    return;
  }
  event.preventDefault();
  const step = event.deltaY < 0 ? 1 : -1;
  zoomToCursor(zoom() + step, event.clientX, event.clientY);
}, { passive: false });

/// Abandon a drag without painting.
function cancelDrag() {
  if (state.drag) {
    state.drag = null;
    drawSelection(null);
  }
}

// A standard install keeps the maps beside `pic.mpq`, so `--pic` already names the directory. Only
// a default in a field the user can change.
(async () => {
  const config = await api("/api/config");
  if (config.ok && config.mapsDirectory) {
    document.getElementById("maps-dir").value = config.mapsDirectory;
    await listDirectory(config.mapsDirectory);
  }
})();

overlay.addEventListener("mousedown", (event) => {
  if (!state.open) {
    return;
  }
  state.drag = cellAt(event);
  drawSelection(normalise(state.drag, state.drag));
});

// A button released outside the window never reaches `mouseup`, so without these the drag stays
// live and the selection goes on tracking the cursor with no button held -- and the next click
// paints a rectangle the user never drew.
window.addEventListener("blur", cancelDrag);

overlay.addEventListener("mousemove", (event) => {
  // The readout is not decoration. Every handler test drives `Editor::handle` directly, so the
  // browser's pixel-to-cell mapping is the one layer nothing exercises -- and a refusal looks
  // exactly like a dead canvas, which is how the first human to use this could not tell a
  // correctly-rejected paint from a broken UI. Showing the cell under the cursor makes the
  // mapping observable rather than inferred.
  if (state.open) {
    const [cx, cy] = cellAt(event);
    readout.textContent = `cell: ${cx}, ${cy}`;
  }
  if (state.drag) {
    // `buttons` is 0 once every button is up, which is how a release that happened outside the
    // window is noticed on the way back in.
    if (event.buttons === 0) {
      cancelDrag();
      return;
    }
    drawSelection(normalise(state.drag, cellAt(event)));
  }
});

overlay.addEventListener("mouseleave", () => {
  readout.textContent = "cell: \u2014";
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
