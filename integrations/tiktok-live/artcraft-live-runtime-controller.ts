import {
  ArtCraftLiveEventDeduper,
  normalizeArtCraftLiveEvent,
  safeArtCraftLiveJsonPreview,
  type ArtCraftLiveCommand,
  type ArtCraftLiveEventKind,
} from './artcraft-live-protocol.js';

const MAX_DEDUP_EVENTS = 10_000;
const MAX_RECONNECT_ATTEMPTS = 5;
const BASE_RECONNECT_DELAY_MS = 500;
const MAX_RECONNECT_DELAY_MS = 30_000;
const MAX_METRIC_COUNT = Number.MAX_SAFE_INTEGER;

function incrementMetricCount(value: number): number {
  return value < MAX_METRIC_COUNT ? value + 1 : MAX_METRIC_COUNT;
}

export interface LiveConnectorPort {
  readonly isConnected?: boolean;
  on(event: string, listener: (value: unknown) => void): unknown;
  connect(): Promise<{ isConnected: boolean; roomId?: string; roomInfo?: unknown; availableGifts?: unknown }>;
  disconnect(): Promise<void>;
  fetchIsLive?(): Promise<boolean>;
  fetchRoomId?(): Promise<string>;
  fetchRoomInfo?(roomId?: string): Promise<unknown>;
  fetchAvailableGifts?(): Promise<unknown>;
  sendMessage?(content: string): Promise<unknown>;
}

export interface LiveConnectorEvents {
  webcast: Record<string, string>;
  disconnected: string;
  error: string;
}

export type LiveRuntimeFrame = Record<string, unknown>;

interface ActiveSession {
  generation: number;
  connection: LiveConnectorPort;
  deduper: ArtCraftLiveEventDeduper;
  reconnectAttempts: number;
  reconnectTimer: ReturnType<typeof setTimeout> | null;
  reconnectInFlight: boolean;
  streamEnded: boolean;
  connectSettled: boolean;
  disconnectedWhileConnecting: boolean;
  disconnectedAfterConnect: boolean;
  disconnectInFlight: Promise<void> | null;
}

function errorCode(error: unknown, fallback: string): string {
  // Upstream ControlEvent.ERROR wraps its Error in { info, exception }.
  if (error !== null && typeof error === 'object' && 'exception' in error) {
    error = (error as { exception: unknown }).exception;
  }
  if (!(error instanceof Error)) return fallback;
  if (error.name === 'PremiumFeatureError') return 'PREMIUM_FEATURE_REQUIRED';
  // Several connector Error subclasses inherit name="Error". Use the actual
  // class when available so offline rooms aren't reported as an opaque ERROR.
  const kind = error.name === 'Error' && error.constructor.name !== 'Error'
    ? error.constructor.name : error.name;
  const name = kind.toUpperCase().replace(/[^A-Z0-9_]/g, '_').slice(0, 64);
  return name || fallback;
}

export class ArtCraftLiveRuntimeController {
  private active: ActiveSession | null = null;
  private generation = 0;
  private shuttingDown = false;
  private droppedEvents = 0;
  private duplicateEvents = 0;

  constructor(
    private readonly createConnection: (username: string) => LiveConnectorPort,
    private readonly events: LiveConnectorEvents,
    private readonly emit: (frame: LiveRuntimeFrame) => void,
  ) {}

  async handle(command: ArtCraftLiveCommand): Promise<void> {
    if (command.type === 'start') {
      await this.start(command);
    } else if (command.type === 'inspect') {
      await this.inspect(command);
    } else if (command.type === 'sendMessage') {
      await this.sendMessage(command);
    } else if (command.type === 'stop') {
      await this.stop(command.requestId);
    } else if (command.type === 'metrics') {
      this.emit({
        type: 'runtime.metrics',
        requestId: command.requestId,
        active: this.active !== null,
        droppedEvents: this.droppedEvents,
        duplicateEvents: this.duplicateEvents,
      });
    } else {
      this.shuttingDown = true;
      await this.stop(command.requestId);
    }
  }

  private async inspect(command: Extract<ArtCraftLiveCommand, { type: 'inspect' }>): Promise<void> {
    let connection: LiveConnectorPort | null = null;
    try {
      connection = this.createConnection(command.username);
      if (!connection.fetchIsLive || !connection.fetchRoomId) throw new Error('INSPECT_UNSUPPORTED');
      const isLive = await connection.fetchIsLive();
      const roomId = await connection.fetchRoomId();
      const [roomInfo, availableGifts] = isLive
        ? await Promise.all([connection.fetchRoomInfo?.(roomId), connection.fetchAvailableGifts?.()])
        : [undefined, undefined];
      this.emit({
        type: 'command.result', requestId: command.requestId, operation: 'inspect',
        username: command.username, isLive, roomId: roomId.slice(0, 128),
        roomInfo: safeArtCraftLiveJsonPreview(roomInfo),
        availableGifts: safeArtCraftLiveJsonPreview(availableGifts),
      });
    } catch (error) {
      this.emit({ type: 'command.error', requestId: command.requestId, code: errorCode(error, 'ROOM_INSPECT_FAILED') });
    } finally {
      if (connection) await connection.disconnect().catch(() => undefined);
    }
  }

  private async sendMessage(command: Extract<ArtCraftLiveCommand, { type: 'sendMessage' }>): Promise<void> {
    const connection = this.active?.connection;
    if (!connection || !connection.sendMessage) {
      this.emit({ type: 'command.error', requestId: command.requestId, code: 'LIVE_SESSION_NOT_CONNECTED' });
      return;
    }
    try {
      await connection.sendMessage(command.content);
      this.emit({ type: 'command.result', requestId: command.requestId, operation: 'sendMessage', sent: true });
    } catch (error) {
      this.emit({ type: 'command.error', requestId: command.requestId, code: errorCode(error, 'LIVE_MESSAGE_SEND_FAILED') });
    }
  }

  private async start(command: Extract<ArtCraftLiveCommand, { type: 'start' }>): Promise<void> {
    if (this.shuttingDown) {
      this.emit({ type: 'command.error', requestId: command.requestId, code: 'RUNTIME_SHUTTING_DOWN' });
      return;
    }
    if (this.active) {
      this.emit({ type: 'command.error', requestId: command.requestId, code: 'LIVE_SESSION_ALREADY_ACTIVE' });
      return;
    }

    let connection: LiveConnectorPort;
    try {
      connection = this.createConnection(command.username);
    } catch (error) {
      this.emit({ type: 'runtime.status', status: 'failed', requestId: command.requestId, code: errorCode(error, 'CONNECTOR_INIT_FAILED') });
      return;
    }

    const session: ActiveSession = {
      generation: ++this.generation,
      connection,
      deduper: new ArtCraftLiveEventDeduper(MAX_DEDUP_EVENTS),
      reconnectAttempts: 0,
      reconnectTimer: null,
      reconnectInFlight: false,
      streamEnded: false,
      connectSettled: false,
      disconnectedWhileConnecting: false,
      disconnectedAfterConnect: false,
      disconnectInFlight: null,
    };
    this.active = session;
    this.attachEventHandlers(session);
    this.emit({ type: 'command.accepted', requestId: command.requestId });
    this.status('connecting', command.requestId);

    try {
      const state = await connection.connect();
      session.connectSettled = true;
      if (this.active?.generation !== session.generation) {
        await this.disconnect(session);
        return;
      }
      this.emitRoomSnapshot(state);
      this.status(state.isConnected ? 'connected' : 'degraded', command.requestId, state.isConnected ? undefined : 'CONNECTOR_NOT_CONNECTED');
      if (!state.isConnected) this.scheduleReconnect(session);
    } catch (error) {
      session.connectSettled = true;
      if (this.active?.generation !== session.generation) {
        await this.disconnect(session);
        return;
      }
      this.active = null;
      await this.disconnect(session);
      this.status('failed', command.requestId, errorCode(error, 'CONNECT_FAILED'));
    }
  }

  private async stop(requestId: string): Promise<void> {
    const session = this.active;
    this.active = null;
    this.generation += 1;
    if (session) {
      if (session.reconnectTimer) clearTimeout(session.reconnectTimer);
      session.reconnectTimer = null;
      await this.disconnect(session);
    }
    this.status('stopped', requestId);
  }

  private async disconnect(session: ActiveSession): Promise<void> {
    // The connector's disconnect() is a no-op while connect() is still doing
    // room/signature setup. If stop races that setup, make one best-effort
    // disconnect now and one after connect settles so a late socket is closed.
    if (session.connectSettled) {
      if (session.disconnectedAfterConnect) return;
      session.disconnectedAfterConnect = true;
    } else {
      if (session.disconnectedWhileConnecting) return;
      session.disconnectedWhileConnecting = true;
    }
    const previousDisconnect = session.disconnectInFlight;
    const disconnect = (async () => {
      if (previousDisconnect) {
        try { await previousDisconnect; } catch { /* continue with the later teardown attempt */ }
      }
      await session.connection.disconnect();
    })();
    session.disconnectInFlight = disconnect;
    try {
      await disconnect;
    } catch {
      // Teardown continues; raw connector errors never cross the local IPC boundary.
    } finally {
      if (session.disconnectInFlight === disconnect) session.disconnectInFlight = null;
    }
  }

  private status(status: string, requestId?: string, code?: string): void {
    this.emit({ type: 'runtime.status', status, ...(requestId ? { requestId } : {}), ...(code ? { code } : {}) });
  }

  private attachEventHandlers(session: ActiveSession): void {
    for (const [kind, eventName] of Object.entries(this.events.webcast) as [ArtCraftLiveEventKind, string][]) {
      session.connection.on(eventName, (data) => {
        if (this.active?.generation !== session.generation) return;
        const event = normalizeArtCraftLiveEvent(kind, data, new Date().toISOString(), `tiktok-live:${session.generation}`);
        if (!event) {
          this.droppedEvents = incrementMetricCount(this.droppedEvents);
          return;
        }
        if (event.dedupeKey !== null && !session.deduper.accept(event.dedupeKey)) {
          this.duplicateEvents = incrementMetricCount(this.duplicateEvents);
          return;
        }
        this.emit(event as unknown as LiveRuntimeFrame);
        if (kind === 'stream_end' || kind === 'streamEnd') session.streamEnded = true;
      });
    }
    session.connection.on(this.events.disconnected, (value) => {
      if (this.active?.generation !== session.generation) return;
      const details = value !== null && typeof value === 'object' ? value as { code?: unknown } : {};
      const closeCode = typeof details.code === 'number' && Number.isFinite(details.code) ? details.code : 0;
      this.status('disconnected', undefined, `WEBSOCKET_CLOSED_${closeCode}`);
      if (session.streamEnded) {
        this.active = null;
        this.status('stopped');
        return;
      }
      this.scheduleReconnect(session);
    });
    session.connection.on(this.events.error, (error) => {
      if (this.active?.generation !== session.generation) return;
      // Decode/processing errors are per-packet, not proof of transport loss.
      // In particular, don't disable AI replies on an otherwise healthy socket.
      if (session.connection.isConnected === true) {
        this.emit({ type: 'runtime.warning', code: errorCode(error, 'CONNECTOR_ERROR') });
        return;
      }
      // Initial/reconnect failures are owned by the awaiting connect() call.
      if (!session.connectSettled || session.reconnectInFlight || session.reconnectTimer) return;
      this.status('degraded', undefined, errorCode(error, 'CONNECTOR_ERROR'));
      if (session.connection.isConnected === false) this.scheduleReconnect(session);
    });
  }

  private scheduleReconnect(session: ActiveSession): void {
    if (this.active?.generation !== session.generation || session.reconnectTimer || session.reconnectInFlight || session.streamEnded) return;
    if (session.reconnectAttempts >= MAX_RECONNECT_ATTEMPTS) {
      this.active = null;
      this.status('failed', undefined, 'LIVE_RECONNECT_EXHAUSTED');
      void this.disconnect(session);
      return;
    }

    session.reconnectAttempts += 1;
    const exponential = Math.min(BASE_RECONNECT_DELAY_MS * 2 ** (session.reconnectAttempts - 1), MAX_RECONNECT_DELAY_MS);
    const delay = Math.min(Math.floor(exponential * (0.5 + Math.random())), MAX_RECONNECT_DELAY_MS);
    this.emit({ type: 'runtime.status', status: 'degraded', code: 'LIVE_RECONNECT_SCHEDULED', reconnectAttempt: session.reconnectAttempts, retryInMs: delay });
    session.reconnectTimer = setTimeout(() => {
      session.reconnectTimer = null;
      void this.reconnect(session);
    }, delay);
  }

  private async reconnect(session: ActiveSession): Promise<void> {
    if (this.active?.generation !== session.generation || session.streamEnded || session.reconnectInFlight) return;
    session.reconnectInFlight = true;
    session.disconnectedAfterConnect = false;
    let recovered = false;
    this.status('connecting', undefined, 'LIVE_RECONNECTING');
    try {
      const state = await session.connection.connect();
      if (this.active?.generation !== session.generation) {
        // stop() may have run while connect() was creating a new socket.
        session.disconnectedAfterConnect = false;
        await this.disconnect(session);
        return;
      }
      if (state.isConnected) {
        recovered = true;
        session.reconnectAttempts = 0;
        this.emitRoomSnapshot(state);
        this.status('connected');
      }
    } catch {
      // Retry is scheduled only after this attempt has fully settled.
    } finally {
      session.reconnectInFlight = false;
      if (!recovered) this.scheduleReconnect(session);
    }
  }

  private emitRoomSnapshot(state: { isConnected: boolean; roomId?: string; roomInfo?: unknown; availableGifts?: unknown }): void {
    this.emit({
      type: 'runtime.room',
      isLive: state.isConnected,
      roomId: typeof state.roomId === 'string' ? state.roomId.slice(0, 128) : null,
      roomInfo: safeArtCraftLiveJsonPreview(state.roomInfo),
      availableGifts: safeArtCraftLiveJsonPreview(state.availableGifts),
    });
  }
}
