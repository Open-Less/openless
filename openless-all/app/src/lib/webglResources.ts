/** Release driver resources on both failed initialization and normal teardown. */
export function releaseWebGL(
  gl: WebGLRenderingContext,
  resources: {
    buffer?: WebGLBuffer | null;
    program?: WebGLProgram | null;
    shaders?: (WebGLShader | null)[];
  } = {},
) {
  if (resources.buffer) gl.deleteBuffer(resources.buffer);
  if (resources.program) gl.deleteProgram(resources.program);
  for (const shader of resources.shaders ?? []) if (shader) gl.deleteShader(shader);
  gl.getExtension('WEBGL_lose_context')?.loseContext();
}
