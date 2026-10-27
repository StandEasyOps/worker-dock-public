export const CURSOR_AGENTS_URL = "https://cursor.com/agents";
export const CURSOR_DOCS_URL =
  "https://cursor.com/docs/cloud-agent/self-hosted/my-machines";
export const STANDEASYOPS_URL = "https://standeasyops.com";

const BC_ID = /^bc-[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;

export function isBcId(value: string): boolean {
  return BC_ID.test(value);
}

export function agentChatUrl(bcId: string): string | null {
  if (!isBcId(bcId)) {
    return null;
  }
  return `${CURSOR_AGENTS_URL}/${bcId}`;
}

export function localReadyzUrl(port: number): string {
  const safe = Number.isInteger(port) && port > 0 && port <= 65535 ? port : 18741;
  return `http://127.0.0.1:${safe}/readyz`;
}
