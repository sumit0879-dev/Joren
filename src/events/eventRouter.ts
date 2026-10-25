import { listen, type UnlistenFn } from '../services/tauri';
import type { EdithEventEnvelope, EdithPayload, EventCategory } from './types';

export type EventHandler<T = EdithPayload> = (event: EdithEventEnvelope<T>) => void;

/**
 * EdithEventRouter
 * Central frontend router for all E.D.I.T.H. typed, correlated IPC events dispatched on "edith-event".
 */
export class JorenEventRouter {
  private categoryHandlers: Map<EventCategory, Set<EventHandler<any>>> = new Map();
  private globalHandlers: Set<EventHandler<EdithPayload>> = new Set();
  private unlistenFns: UnlistenFn[] = [];
  private isListening = false;

  constructor() {
    this.categoryHandlers.set('stream', new Set());
    this.categoryHandlers.set('task', new Set());
    this.categoryHandlers.set('tool', new Set());
    this.categoryHandlers.set('voice', new Set());
    this.categoryHandlers.set('runtime', new Set());
    this.categoryHandlers.set('security_policy', new Set());
  }

  /**
   * Initializes the Tauri IPC listener for 'joren-event' with 'edith-event' compatibility fallback.
   * Safe to call multiple times (idempotent).
   */
  public async init(): Promise<void> {
    if (this.isListening) return;

    try {
      const u1 = await listen<EdithEventEnvelope<EdithPayload>>('joren-event', (event) => {
        this.dispatch(event.payload);
      });
      const u2 = await listen<EdithEventEnvelope<EdithPayload>>('edith-event', (event) => {
        this.dispatch(event.payload);
      });
      this.unlistenFns = [u1, u2];
      this.isListening = true;
    } catch (err) {
      console.warn('[JorenEventRouter] Failed to attach Tauri IPC listener:', err);
    }
  }

  /**
   * Dispatches an incoming event to matching category and global handlers.
   * Defensively wraps handler calls so an exception in one handler never disrupts others.
   */
  public dispatch(envelope: EdithEventEnvelope<EdithPayload>): void {
    if (!envelope || !envelope.payload) return;

    const category = envelope.payload.category;
    const categorySet = this.categoryHandlers.get(category);

    if (categorySet) {
      for (const handler of categorySet) {
        try {
          handler(envelope);
        } catch (err) {
          console.error(`[EdithEventRouter] Error in ${category} event handler:`, err);
        }
      }
    }

    for (const handler of this.globalHandlers) {
      try {
        handler(envelope);
      } catch (err) {
        console.error('[EdithEventRouter] Error in global event handler:', err);
      }
    }
  }

  /**
   * Subscribe to events of a specific category.
   * Returns an unsubscribe function.
   */
  public onCategory<K extends EventCategory>(
    category: K,
    handler: EventHandler<Extract<EdithPayload, { category: K }>>
  ): () => void {
    this.init();
    const set = this.categoryHandlers.get(category);
    if (set) {
      set.add(handler as EventHandler<any>);
    }

    return () => {
      set?.delete(handler as EventHandler<any>);
    };
  }

  /**
   * Subscribe to all events regardless of category.
   * Returns an unsubscribe function.
   */
  public onAny(handler: EventHandler<EdithPayload>): () => void {
    this.init();
    this.globalHandlers.add(handler);
    return () => {
      this.globalHandlers.delete(handler);
    };
  }

  /**
   * Clean up all listeners.
   */
  public destroy(): void {
    for (const u of this.unlistenFns) {
      try {
        u();
      } catch (_) {}
    }
    this.unlistenFns = [];
    this.isListening = false;
    this.categoryHandlers.forEach((set) => set.clear());
    this.globalHandlers.clear();
  }
}

export const jorenEventRouter = new JorenEventRouter();
export const eventRouter = jorenEventRouter;
export const edithEventRouter = jorenEventRouter;
export const EdithEventRouter = JorenEventRouter;
