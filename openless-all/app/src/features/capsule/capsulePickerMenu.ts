/**
 * Session-bound lifecycle shared by the capsule's language and style pickers.
 *
 * The native capsule owns one temporary headroom reservation per recording
 * session (`set_capsule_language_menu` / `set_capsule_style_menu`). This
 * controller is the only place which reserves, queues and releases it, so every
 * picker gets the same ordering, abort and session-invalidation guarantees.
 */

export interface CapsulePickerTarget {
  sessionId: string;
  mutable: boolean;
}

export type CapsulePickerKind = 'language' | 'style';

export interface CapsulePickerOwner {
  kind: CapsulePickerKind;
  sessionId: string;
  requestId?: string;
}

export interface CapsulePickerDeps {
  /** Reserve (`true`) or restore (`false`) the native headroom for a session. */
  setNativeMenu: (open: boolean, owner: CapsulePickerOwner) => Promise<void>;
  /** Whether a native host owns the geometry; the browser preview has none. */
  native: boolean;
  /** Resolves once the anchor and viewport can hold the popup. */
  waitForGeometry: (signal: AbortSignal) => Promise<void>;
  /** Mirrors the mounted state into the renderer. */
  onOpenChange: (open: boolean) => void;
  /** An opening request failed while it was still the current one. */
  onFailure: () => void;
}

interface Reservation extends CapsulePickerOwner {
  deps: CapsulePickerDeps;
  abort: AbortController;
  nativeWriteStarted: boolean;
  mounted: boolean;
}

export class CapsulePickerMenu {
  private reservation: Reservation | undefined;
  private exiting: Reservation | undefined;
  private tail: Promise<void> = Promise.resolve();

  owns(kind: CapsulePickerKind, sessionId: string, requestId: string) {
    const owner = this.reservation;
    return owner?.kind === kind && owner.sessionId === sessionId && owner.requestId === requestId;
  }

  /** The session whose popup is open or still animating out. */
  sessionForChoice(kind: CapsulePickerKind) {
    const owner = this.reservation ?? this.exiting;
    return owner?.kind === kind ? owner.sessionId : undefined;
  }

  update(kind: CapsulePickerKind, visible: boolean, target: CapsulePickerTarget | null) {
    const owner = this.reservation;
    if (
      owner?.kind === kind &&
      (!visible || !target?.mutable || target.sessionId !== owner.sessionId)
    ) {
      this.close(kind);
    }
  }

  async open(request: CapsulePickerOwner, deps: CapsulePickerDeps) {
    const previous = this.reservation;
    if (
      previous?.kind === request.kind &&
      previous.sessionId === request.sessionId &&
      previous.requestId === request.requestId
    ) {
      return;
    }
    if (previous) this.close(previous.kind);
    // The host shares one reservation for both pickers. Release the old owner
    // before reserving for a different picker or opening in the same session again.
    if (this.exiting) this.release(this.exiting);
    const owner: Reservation = {
      ...request,
      deps,
      abort: new AbortController(),
      nativeWriteStarted: false,
      mounted: false,
    };
    this.reservation = owner;
    try {
      if (deps.native) {
        await this.enqueue(async () => {
          if (owner.abort.signal.aborted) return;
          owner.nativeWriteStarted = true;
          await deps.setNativeMenu(true, owner);
        });
        await deps.waitForGeometry(owner.abort.signal);
      }
      if (this.reservation !== owner || owner.abort.signal.aborted) return;
      owner.mounted = true;
      deps.onOpenChange(true);
    } catch {
      if (this.reservation === owner && !owner.abort.signal.aborted) {
        this.close(owner.kind);
        deps.onFailure();
      }
    }
  }

  close(kind: CapsulePickerKind) {
    const owner = this.reservation;
    if (owner?.kind !== kind) return;
    this.reservation = undefined;
    owner.abort.abort();
    owner.deps.onOpenChange(false);
    // The style popup has no exit animation and must release native keys immediately.
    if (owner.mounted && kind === 'language') this.exiting = owner;
    else this.release(owner);
  }

  /** Base UI finished its exit transition; the native headroom can be restored. */
  openChangeComplete(kind: CapsulePickerKind, next: boolean) {
    if (!next && this.exiting?.kind === kind) this.release(this.exiting);
  }

  /** Escape before the popup mounted; once mounted, Base UI owns it. */
  handleKeydown(
    kind: CapsulePickerKind,
    event: { key: string; isComposing?: boolean; preventDefault: () => void },
  ) {
    if (
      event.key === 'Escape' &&
      !event.isComposing &&
      this.reservation?.kind === kind &&
      !this.reservation.mounted
    ) {
      event.preventDefault();
      this.close(kind);
    }
  }

  dispose(kind: CapsulePickerKind) {
    if (this.reservation?.kind === kind) {
      this.release(this.reservation);
      this.reservation = undefined;
    }
    if (this.exiting?.kind === kind) this.release(this.exiting);
  }

  private enqueue(operation: () => Promise<void>) {
    const result = this.tail.catch(() => undefined).then(operation);
    this.tail = result;
    return result;
  }

  private release(owner: Reservation) {
    if (this.exiting === owner) this.exiting = undefined;
    owner.abort.abort();
    void this.enqueue(async () => {
      if (owner.nativeWriteStarted) {
        owner.nativeWriteStarted = false;
        await owner.deps.setNativeMenu(false, owner);
      }
    }).catch((error) =>
      console.warn('[capsule] failed to restore picker menu geometry', error),
    );
  }
}
