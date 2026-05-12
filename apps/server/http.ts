const JSON_HEADERS = {
  "Content-Type": "application/json",
  "Access-Control-Allow-Origin": process.env.CORS_ORIGIN ?? "*",
  "Access-Control-Allow-Methods": "GET,POST,PUT,DELETE,OPTIONS",
  "Access-Control-Allow-Headers": "Content-Type,Authorization",
};

export function json(data: unknown, status = 200): Response {
  return new Response(JSON.stringify(data), {
    status,
    headers: JSON_HEADERS,
  });
}

export function noContent(): Response {
  return new Response(null, {
    status: 204,
    headers: JSON_HEADERS,
  });
}

export function error(message: string, status = 400): Response {
  return json({ error: message }, status);
}

export function withCors(response: Response): Response {
  const headers = new Headers(response.headers);
  headers.set("Access-Control-Allow-Origin", JSON_HEADERS["Access-Control-Allow-Origin"]);
  headers.set("Access-Control-Allow-Methods", JSON_HEADERS["Access-Control-Allow-Methods"]);
  headers.set("Access-Control-Allow-Headers", JSON_HEADERS["Access-Control-Allow-Headers"]);

  return new Response(response.body, {
    status: response.status,
    statusText: response.statusText,
    headers,
  });
}
