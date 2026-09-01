const MAX_CANVAS_PIXELS = 3_000_000;

export function selectEffectsMode({ supportsWebGL, reducedMotion, saveData }) {
  return supportsWebGL && !reducedMotion && !saveData ? "animated" : "static";
}

export function getCanvasSize(
  width,
  height,
  devicePixelRatio = 1,
  maxPixels = MAX_CANVAS_PIXELS,
) {
  const safeWidth = Math.max(1, Number(width) || 1);
  const safeHeight = Math.max(1, Number(height) || 1);
  const preferredRatio = Math.min(1.5, Math.max(1, Number(devicePixelRatio) || 1));
  const preferredPixels = safeWidth * safeHeight * preferredRatio ** 2;
  const budgetScale = Math.min(1, Math.sqrt(maxPixels / preferredPixels));
  const pixelRatio = preferredRatio * budgetScale;

  return {
    width: Math.max(1, Math.floor(safeWidth * pixelRatio)),
    height: Math.max(1, Math.floor(safeHeight * pixelRatio)),
    pixelRatio,
  };
}

function createShader(gl, type, source) {
  const shader = gl.createShader(type);
  if (!shader) return null;

  gl.shaderSource(shader, source);
  gl.compileShader(shader);

  if (!gl.getShaderParameter(shader, gl.COMPILE_STATUS)) {
    gl.deleteShader(shader);
    return null;
  }

  return shader;
}

function startAmbientCanvas(canvas) {
  const gl = canvas.getContext("webgl", {
    alpha: true,
    antialias: false,
    powerPreference: "low-power",
  });
  if (!gl) return false;

  const vertexSource = `
    attribute vec2 position;
    void main() {
      gl_Position = vec4(position, 0.0, 1.0);
    }
  `;

  const fragmentSource = `
    precision mediump float;
    uniform vec2 resolution;
    uniform vec2 pointer;
    uniform float time;

    float glow(vec2 uv, vec2 origin, float radius) {
      return 1.0 - smoothstep(0.0, radius, distance(uv, origin));
    }

    void main() {
      vec2 uv = gl_FragCoord.xy / resolution.xy;
      vec2 cursor = pointer / resolution.xy;
      float aspect = resolution.x / resolution.y;
      uv.x *= aspect;
      cursor.x *= aspect;

      float drift = sin((uv.x + uv.y) * 3.4 + time * 0.12) * 0.035;
      float cursorGlow = glow(uv, cursor, 0.72) * 0.16;
      float heroGlow = glow(uv, vec2(aspect * 0.58, 0.74 + drift), 1.05) * 0.22;
      float edgeGlow = glow(uv, vec2(aspect * 0.98, 0.32 - drift), 0.82) * 0.10;
      float intensity = cursorGlow + heroGlow + edgeGlow;

      vec3 lime = vec3(0.64, 0.90, 0.21);
      vec3 cool = vec3(0.19, 0.31, 0.28);
      vec3 color = mix(cool, lime, 0.64) * intensity;
      gl_FragColor = vec4(color, intensity * 0.82);
    }
  `;

  const vertexShader = createShader(gl, gl.VERTEX_SHADER, vertexSource);
  const fragmentShader = createShader(gl, gl.FRAGMENT_SHADER, fragmentSource);
  if (!vertexShader || !fragmentShader) return false;

  const program = gl.createProgram();
  if (!program) return false;
  gl.attachShader(program, vertexShader);
  gl.attachShader(program, fragmentShader);
  gl.linkProgram(program);

  if (!gl.getProgramParameter(program, gl.LINK_STATUS)) return false;

  const buffer = gl.createBuffer();
  if (!buffer) return false;
  gl.bindBuffer(gl.ARRAY_BUFFER, buffer);
  gl.bufferData(
    gl.ARRAY_BUFFER,
    new Float32Array([-1, -1, 1, -1, -1, 1, -1, 1, 1, -1, 1, 1]),
    gl.STATIC_DRAW,
  );

  gl.useProgram(program);
  const position = gl.getAttribLocation(program, "position");
  gl.enableVertexAttribArray(position);
  gl.vertexAttribPointer(position, 2, gl.FLOAT, false, 0, 0);

  const resolution = gl.getUniformLocation(program, "resolution");
  const pointer = gl.getUniformLocation(program, "pointer");
  const time = gl.getUniformLocation(program, "time");
  let pointerX = window.innerWidth * 0.62;
  let pointerY = window.innerHeight * 0.72;
  let frame = 0;
  let lastRendered = 0;
  const startedAt = performance.now();

  function resize() {
    const size = getCanvasSize(
      window.innerWidth,
      window.innerHeight,
      window.devicePixelRatio,
    );
    canvas.width = size.width;
    canvas.height = size.height;
    gl.viewport(0, 0, size.width, size.height);
  }

  function render(now) {
    if (now - lastRendered < 1000 / 30) {
      frame = window.requestAnimationFrame(render);
      return;
    }
    lastRendered = now;
    const scaleX = canvas.width / window.innerWidth;
    const scaleY = canvas.height / window.innerHeight;
    gl.uniform2f(resolution, canvas.width, canvas.height);
    gl.uniform2f(pointer, pointerX * scaleX, (window.innerHeight - pointerY) * scaleY);
    gl.uniform1f(time, (now - startedAt) / 1000);
    gl.drawArrays(gl.TRIANGLES, 0, 6);
    frame = window.requestAnimationFrame(render);
  }

  resize();
  window.addEventListener("resize", resize, { passive: true });
  window.addEventListener(
    "pointermove",
    (event) => {
      pointerX = event.clientX;
      pointerY = event.clientY;
    },
    { passive: true },
  );
  document.addEventListener("visibilitychange", () => {
    if (document.hidden) {
      window.cancelAnimationFrame(frame);
    } else {
      frame = window.requestAnimationFrame(render);
    }
  });
  frame = window.requestAnimationFrame(render);
  return true;
}

function setupSpotlights() {
  if (!window.matchMedia("(hover: hover) and (pointer: fine)").matches) return;

  for (const card of document.querySelectorAll("[data-spotlight]")) {
    card.addEventListener(
      "pointermove",
      (event) => {
        const bounds = card.getBoundingClientRect();
        card.style.setProperty("--pointer-x", `${event.clientX - bounds.left}px`);
        card.style.setProperty("--pointer-y", `${event.clientY - bounds.top}px`);
      },
      { passive: true },
    );
  }
}

function setupReveal() {
  const items = document.querySelectorAll("[data-reveal]");
  if (!("IntersectionObserver" in window)) {
    for (const item of items) item.classList.add("is-visible");
    return;
  }

  const observer = new IntersectionObserver(
    (entries) => {
      for (const entry of entries) {
        if (!entry.isIntersecting) continue;
        entry.target.classList.add("is-visible");
        observer.unobserve(entry.target);
      }
    },
    { rootMargin: "0px 0px -8%", threshold: 0.12 },
  );

  for (const item of items) observer.observe(item);
}

function boot() {
  const canvas = document.querySelector("#ambient-canvas");
  const reducedMotion = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
  const saveData = Boolean(navigator.connection?.saveData);
  const supportsWebGL = Boolean(canvas?.getContext);
  const mode = selectEffectsMode({ supportsWebGL, reducedMotion, saveData });
  document.documentElement.dataset.effects = mode;

  if (mode === "animated" && canvas && !startAmbientCanvas(canvas)) {
    document.documentElement.dataset.effects = "static";
  }

  setupSpotlights();
  setupReveal();

  const year = document.querySelector("[data-current-year]");
  if (year) year.textContent = String(new Date().getFullYear());
}

if (typeof document !== "undefined") {
  boot();
}
