export const previewPort = Number(process.env.OPENLESS_E2E_PREVIEW_PORT ?? 5325);
export const fixturePort = Number(process.env.OPENLESS_E2E_FIXTURE_PORT ?? 5326);

export const previewOrigin = `http://127.0.0.1:${previewPort}`;
export const fixtureOrigin = `http://127.0.0.1:${fixturePort}`;
