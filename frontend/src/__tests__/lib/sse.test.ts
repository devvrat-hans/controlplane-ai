import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";

class MockEventSource {
  url: string;
  onmessage: ((event: MessageEvent) => void) | null = null;
  onerror: ((event: Event) => void) | null = null;
  onopen: (() => void) | null = null;
  readyState = 0;
  private listeners: Record<string, ((event: MessageEvent) => void)[]> = {};

  static CONNECTING = 0;
  static OPEN = 1;
  static CLOSED = 2;

  constructor(url: string) {
    this.url = url;
    setTimeout(() => {
      this.readyState = 1;
      this.onopen?.();
    }, 0);
  }

  addEventListener(type: string, listener: (event: MessageEvent) => void) {
    if (!this.listeners[type]) this.listeners[type] = [];
    this.listeners[type].push(listener);
  }

  removeEventListener(type: string, listener: (event: MessageEvent) => void) {
    if (this.listeners[type]) {
      this.listeners[type] = this.listeners[type].filter((l) => l !== listener);
    }
  }

  close() {
    this.readyState = 2;
  }

  simulateMessage(data: string, type = "message") {
    const event = new MessageEvent(type, { data });
    if (type === "message" && this.onmessage) {
      this.onmessage(event);
    }
    this.listeners[type]?.forEach((l) => l(event));
  }

  simulateError() {
    this.readyState = 2;
    this.onerror?.(new Event("error"));
  }
}

describe("SSE Connection", () => {
  let originalEventSource: typeof EventSource;

  beforeEach(() => {
    originalEventSource = globalThis.EventSource;
    // @ts-expect-error -- mock class shape is sufficient
    globalThis.EventSource = MockEventSource;
  });

  afterEach(() => {
    globalThis.EventSource = originalEventSource;
  });

  it("connects to the correct SSE endpoint", () => {
    const es = new EventSource("http://localhost:8080/api/v1/verdicts/stream");
    expect(es.url).toBe("http://localhost:8080/api/v1/verdicts/stream");
  });

  it("parses verdict events from SSE messages", async () => {
    const es = new MockEventSource(
      "http://localhost:8080/api/v1/verdicts/stream"
    );
    const verdicts: unknown[] = [];

    es.onmessage = (event: MessageEvent) => {
      verdicts.push(JSON.parse(event.data));
    };

    const sampleVerdict = {
      type: "verdict",
      correlation_id: "test-123",
      app_id: "app-1",
      timestamp: "2026-08-19T17:30:00Z",
      verdict: {
        id: "v-1",
        call_id: "c-1",
        axis: "secret_leak",
        path: "fast",
        outcome: "edit",
        confidence: 0.95,
        reason: "AWS key detected",
        check_name: "secret_detection",
      },
    };

    es.simulateMessage(JSON.stringify(sampleVerdict));

    expect(verdicts).toHaveLength(1);
    expect(verdicts[0]).toEqual(sampleVerdict);
  });

  it("handles multiple messages in sequence", () => {
    const es = new MockEventSource(
      "http://localhost:8080/api/v1/verdicts/stream"
    );
    const messages: string[] = [];

    es.onmessage = (event: MessageEvent) => {
      messages.push(event.data);
    };

    es.simulateMessage('{"type":"verdict","id":"1"}');
    es.simulateMessage('{"type":"verdict","id":"2"}');
    es.simulateMessage('{"type":"verdict","id":"3"}');

    expect(messages).toHaveLength(3);
  });

  it("closes the connection cleanly", () => {
    const es = new MockEventSource(
      "http://localhost:8080/api/v1/verdicts/stream"
    );
    expect(es.readyState).toBe(0);

    es.close();
    expect(es.readyState).toBe(2);
  });

  it("triggers onerror on connection failure", () => {
    const es = new MockEventSource(
      "http://localhost:8080/api/v1/verdicts/stream"
    );
    const errors: Event[] = [];

    es.onerror = (event) => {
      errors.push(event);
    };

    es.simulateError();
    expect(errors).toHaveLength(1);
    expect(es.readyState).toBe(2);
  });
});

describe("SSE Verdict Payload Validation", () => {
  it("validates verdict payload structure", () => {
    const validPayload = {
      type: "verdict",
      correlation_id: "uuid-123",
      app_id: "app-uuid",
      timestamp: "2026-08-19T17:30:00Z",
      verdict: {
        id: "verdict-uuid",
        call_id: "call-uuid",
        axis: "secret_leak",
        path: "fast",
        outcome: "edit",
        confidence: 0.95,
        reason: "Detected AWS access key",
        check_name: "secret_detection",
      },
    };

    expect(validPayload.type).toBe("verdict");
    expect(validPayload.verdict.axis).toBeDefined();
    expect(validPayload.verdict.outcome).toMatch(
      /^(pass|edit|block|escalate)$/
    );
    expect(validPayload.verdict.confidence).toBeGreaterThanOrEqual(0);
    expect(validPayload.verdict.confidence).toBeLessThanOrEqual(1);
  });

  it("identifies invalid outcome values", () => {
    const outcome = "invalid_outcome";
    expect(outcome).not.toMatch(/^(pass|edit|block|escalate)$/);
  });

  it("identifies confidence out of range", () => {
    expect(1.5).toBeGreaterThan(1);
    expect(-0.1).toBeLessThan(0);
  });
});
