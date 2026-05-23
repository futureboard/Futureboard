import type { GridLine, GridLineLevel } from "../../utils/musicalGrid";

type GpuContext = WebGLRenderingContext | WebGL2RenderingContext;

const VERT = `
attribute vec2 a_pos;
attribute vec4 a_color;
uniform vec2 u_resolution;
varying vec4 v_color;
void main() {
  vec2 zeroToOne = a_pos / u_resolution;
  vec2 clip = zeroToOne * 2.0 - 1.0;
  gl_Position = vec4(clip * vec2(1.0, -1.0), 0.0, 1.0);
  v_color = a_color;
}
`;

const FRAG = `
precision mediump float;
varying vec4 v_color;
void main() {
  gl_FragColor = v_color;
}
`;

const GRID_ALPHA: Record<GridLineLevel, number> = {
  bar: 0.14,
  beat: 0.062,
  sub: 0.026,
};

export class TimelineGpuGridRenderer {
  private gl: GpuContext;
  private program: WebGLProgram;
  private buffer: WebGLBuffer;
  private posLoc: number;
  private colorLoc: number;
  private resolutionLoc: WebGLUniformLocation;
  private vertices = new Float32Array(8192);
  private vertexFloats = 0;
  private dpr = 1;
  private width = 1;
  private height = 1;

  static create(canvas: HTMLCanvasElement): TimelineGpuGridRenderer | null {
    const gl = (
      canvas.getContext("webgl2", { alpha: true, antialias: false, depth: false, stencil: false, preserveDrawingBuffer: false }) ??
      canvas.getContext("webgl", { alpha: true, antialias: false, depth: false, stencil: false, preserveDrawingBuffer: false })
    ) as GpuContext | null;
    if (!gl) return null;
    canvas.addEventListener("webglcontextlost", (event) => {
      event.preventDefault();
      canvas.style.display = "none";
      console.warn("[TimelineGPU] Grid WebGL context lost; hiding GPU grid surface.");
    }, { once: true });

    try {
      return new TimelineGpuGridRenderer(gl);
    } catch (error) {
      console.warn("[TimelineGPU] WebGL grid renderer failed to initialize:", error);
      return null;
    }
  }

  private constructor(gl: GpuContext) {
    this.gl = gl;
    const program = createProgram(gl, VERT, FRAG);
    const buffer = gl.createBuffer();
    const resolutionLoc = gl.getUniformLocation(program, "u_resolution");
    if (!buffer || !resolutionLoc) throw new Error("WebGL resources unavailable");

    this.program = program;
    this.buffer = buffer;
    this.posLoc = gl.getAttribLocation(program, "a_pos");
    this.colorLoc = gl.getAttribLocation(program, "a_color");
    this.resolutionLoc = resolutionLoc;

    gl.useProgram(program);
    gl.bindBuffer(gl.ARRAY_BUFFER, buffer);
    gl.enableVertexAttribArray(this.posLoc);
    gl.enableVertexAttribArray(this.colorLoc);
    gl.vertexAttribPointer(this.posLoc, 2, gl.FLOAT, false, 24, 0);
    gl.vertexAttribPointer(this.colorLoc, 4, gl.FLOAT, false, 24, 8);
  }

  resize(width: number, height: number, dpr: number): void {
    this.width = Math.max(1, width);
    this.height = Math.max(1, height);
    this.dpr = Math.max(1, Math.min(2, dpr || 1));

    const canvas = this.gl.canvas as HTMLCanvasElement;
    if (this.gl.isContextLost()) throw new Error("WebGL context lost");
    const bw = Math.ceil(this.width * this.dpr);
    const bh = Math.ceil(this.height * this.dpr);
    if (canvas.width !== bw || canvas.height !== bh) {
      canvas.width = bw;
      canvas.height = bh;
    }
    canvas.style.width = `${this.width}px`;
    canvas.style.height = `${this.height}px`;
    canvas.style.display = "block";
    this.gl.viewport(0, 0, bw, bh);
  }

  render(lines: GridLine[], scrollX: number, ppb: number, bpb: number): void {
    const gl = this.gl;
    if (gl.isContextLost()) throw new Error("WebGL context lost");
    gl.useProgram(this.program);
    gl.bindBuffer(gl.ARRAY_BUFFER, this.buffer);
    gl.disable(gl.DEPTH_TEST);
    gl.disable(gl.STENCIL_TEST);
    gl.enable(gl.BLEND);
    gl.blendFunc(gl.SRC_ALPHA, gl.ONE_MINUS_SRC_ALPHA);
    gl.uniform2f(this.resolutionLoc, this.width, this.height);
    gl.clearColor(0, 0, 0, 0);
    gl.clear(gl.COLOR_BUFFER_BIT);

    this.vertexFloats = 0;
    this.pushBarShading(scrollX, ppb, bpb);

    for (const line of lines) {
      if (line.level !== "bar") this.pushLine(line.x, GRID_ALPHA[line.level]);
    }
    for (const line of lines) {
      if (line.level === "bar") this.pushLine(line.x, GRID_ALPHA.bar);
    }

    if (this.vertexFloats === 0) return;

    gl.bufferData(gl.ARRAY_BUFFER, this.vertices.subarray(0, this.vertexFloats), gl.DYNAMIC_DRAW);
    gl.vertexAttribPointer(this.posLoc, 2, gl.FLOAT, false, 24, 0);
    gl.vertexAttribPointer(this.colorLoc, 4, gl.FLOAT, false, 24, 8);
    gl.drawArrays(gl.TRIANGLES, 0, this.vertexFloats / 6);
  }

  dispose(): void {
    const gl = this.gl;
    gl.deleteBuffer(this.buffer);
    gl.deleteProgram(this.program);
  }

  private pushBarShading(scrollX: number, ppb: number, bpb: number): void {
    const barW = bpb * ppb;
    if (barW < 2) return;

    const firstBar = Math.floor((scrollX / ppb) / bpb);
    for (let bar = firstBar; bar * barW - scrollX < this.width + barW; bar++) {
      if (bar % 2 !== 0) continue;
      const x = Math.round(bar * barW - scrollX);
      this.pushRect(x, 0, Math.round(barW), this.height, 1, 1, 1, 0.022);
    }
  }

  private pushLine(x: number, alpha: number): void {
    this.pushRect(Math.round(x), 0, 1, this.height, 1, 1, 1, alpha);
  }

  private pushRect(x: number, y: number, w: number, h: number, r: number, g: number, b: number, a: number): void {
    if (w <= 0 || h <= 0 || x > this.width || x + w < 0) return;
    this.ensure(36);
    const x0 = x;
    const y0 = y;
    const x1 = x + w;
    const y1 = y + h;
    this.pushVertex(x0, y0, r, g, b, a);
    this.pushVertex(x1, y0, r, g, b, a);
    this.pushVertex(x0, y1, r, g, b, a);
    this.pushVertex(x0, y1, r, g, b, a);
    this.pushVertex(x1, y0, r, g, b, a);
    this.pushVertex(x1, y1, r, g, b, a);
  }

  private pushVertex(x: number, y: number, r: number, g: number, b: number, a: number): void {
    const i = this.vertexFloats;
    this.vertices[i] = x;
    this.vertices[i + 1] = y;
    this.vertices[i + 2] = r;
    this.vertices[i + 3] = g;
    this.vertices[i + 4] = b;
    this.vertices[i + 5] = a;
    this.vertexFloats += 6;
  }

  private ensure(extraFloats: number): void {
    if (this.vertexFloats + extraFloats <= this.vertices.length) return;
    const next = new Float32Array(Math.max(this.vertices.length * 2, this.vertexFloats + extraFloats));
    next.set(this.vertices);
    this.vertices = next;
  }
}

function createShader(gl: GpuContext, type: number, source: string): WebGLShader {
  const shader = gl.createShader(type);
  if (!shader) throw new Error("Unable to create shader");
  gl.shaderSource(shader, source);
  gl.compileShader(shader);
  if (!gl.getShaderParameter(shader, gl.COMPILE_STATUS)) {
    const log = gl.getShaderInfoLog(shader) ?? "unknown shader error";
    gl.deleteShader(shader);
    throw new Error(log);
  }
  return shader;
}

function createProgram(gl: GpuContext, vert: string, frag: string): WebGLProgram {
  const vs = createShader(gl, gl.VERTEX_SHADER, vert);
  const fs = createShader(gl, gl.FRAGMENT_SHADER, frag);
  const program = gl.createProgram();
  if (!program) throw new Error("Unable to create program");
  gl.attachShader(program, vs);
  gl.attachShader(program, fs);
  gl.linkProgram(program);
  gl.deleteShader(vs);
  gl.deleteShader(fs);
  if (!gl.getProgramParameter(program, gl.LINK_STATUS)) {
    const log = gl.getProgramInfoLog(program) ?? "unknown program error";
    gl.deleteProgram(program);
    throw new Error(log);
  }
  return program;
}
