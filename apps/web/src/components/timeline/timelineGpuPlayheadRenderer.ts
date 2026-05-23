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

const ACCENT = { r: 0.3725, g: 0.8078, b: 0.8157 };

export class TimelineGpuPlayheadRenderer {
  private gl: GpuContext;
  private program: WebGLProgram;
  private buffer: WebGLBuffer;
  private posLoc: number;
  private colorLoc: number;
  private resolutionLoc: WebGLUniformLocation;
  private width = 1;
  private height = 1;
  private vertices = new Float32Array(54);
  private vertexFloats = 0;

  static create(canvas: HTMLCanvasElement): TimelineGpuPlayheadRenderer | null {
    const gl = (
      canvas.getContext("webgl2", { alpha: true, antialias: false, depth: false, stencil: false, preserveDrawingBuffer: false }) ??
      canvas.getContext("webgl", { alpha: true, antialias: false, depth: false, stencil: false, preserveDrawingBuffer: false })
    ) as GpuContext | null;
    if (!gl) return null;
    canvas.addEventListener("webglcontextlost", (event) => {
      event.preventDefault();
      canvas.style.display = "none";
      console.warn("[TimelineGPU] Playhead WebGL context lost; hiding GPU playhead surface.");
    }, { once: true });
    try {
      return new TimelineGpuPlayheadRenderer(gl);
    } catch (error) {
      console.warn("[TimelineGPU] Playhead renderer failed to initialize:", error);
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
  }

  resize(width: number, height: number, dpr: number): void {
    this.width = Math.max(1, width);
    this.height = Math.max(1, height);
    const ratio = Math.max(1, Math.min(2, dpr || 1));
    const canvas = this.gl.canvas as HTMLCanvasElement;
    if (this.gl.isContextLost()) throw new Error("WebGL context lost");
    const bw = Math.ceil(this.width * ratio);
    const bh = Math.ceil(this.height * ratio);
    if (canvas.width !== bw || canvas.height !== bh) {
      canvas.width = bw;
      canvas.height = bh;
    }
    canvas.style.width = `${this.width}px`;
    canvas.style.height = `${this.height}px`;
    canvas.style.display = "block";
    this.gl.viewport(0, 0, bw, bh);
  }

  render(x: number): void {
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
    this.pushRect(Math.round(x) - 1, 0, 2, this.height, 0.82);
    this.pushTriangle(Math.round(x), 0, 12, 12, 1);

    gl.bufferData(gl.ARRAY_BUFFER, this.vertices.subarray(0, this.vertexFloats), gl.DYNAMIC_DRAW);
    gl.enableVertexAttribArray(this.posLoc);
    gl.enableVertexAttribArray(this.colorLoc);
    gl.vertexAttribPointer(this.posLoc, 2, gl.FLOAT, false, 24, 0);
    gl.vertexAttribPointer(this.colorLoc, 4, gl.FLOAT, false, 24, 8);
    gl.drawArrays(gl.TRIANGLES, 0, this.vertexFloats / 6);
  }

  dispose(): void {
    this.gl.deleteBuffer(this.buffer);
    this.gl.deleteProgram(this.program);
  }

  private pushRect(x: number, y: number, w: number, h: number, a: number): void {
    const x0 = x;
    const y0 = y;
    const x1 = x + w;
    const y1 = y + h;
    this.pushVertex(x0, y0, a);
    this.pushVertex(x1, y0, a);
    this.pushVertex(x0, y1, a);
    this.pushVertex(x0, y1, a);
    this.pushVertex(x1, y0, a);
    this.pushVertex(x1, y1, a);
  }

  private pushTriangle(cx: number, y: number, w: number, h: number, a: number): void {
    this.pushVertex(cx - w / 2, y, a);
    this.pushVertex(cx + w / 2, y, a);
    this.pushVertex(cx, y + h, a);
  }

  private pushVertex(x: number, y: number, a: number): void {
    const i = this.vertexFloats;
    this.vertices[i] = x;
    this.vertices[i + 1] = y;
    this.vertices[i + 2] = ACCENT.r;
    this.vertices[i + 3] = ACCENT.g;
    this.vertices[i + 4] = ACCENT.b;
    this.vertices[i + 5] = a;
    this.vertexFloats += 6;
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
