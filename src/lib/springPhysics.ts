/**
 * Damped harmonic oscillator for spring-physics cursor animation.
 * Matched from Clicky Web: stiffness=170, damping=18.
 */
export class SpringValue {
  position: number;
  velocity = 0;
  target: number;
  private stiffness: number;
  private damping: number;

  constructor(initial: number, stiffness = 170, damping = 18) {
    this.position = initial;
    this.target = initial;
    this.stiffness = stiffness;
    this.damping = damping;
  }

  setTarget(target: number): void {
    this.target = target;
  }

  /**
   * Advance the spring by `dt` seconds.
   * Returns true if still animating (not settled).
   */
  step(dt: number): boolean {
    if (dt <= 0) return !this.isSettled();
    const force = this.stiffness * (this.target - this.position);
    const dampingForce = this.damping * this.velocity;
    const acceleration = force - dampingForce;
    this.velocity += acceleration * dt;
    this.position += this.velocity * dt;

    // Settled when close to target with low velocity
    return Math.abs(this.target - this.position) > 0.5 || Math.abs(this.velocity) > 0.5;
  }

  isSettled(): boolean {
    return Math.abs(this.target - this.position) < 0.5 && Math.abs(this.velocity) < 0.5;
  }
}

/**
 * Animate along a quadratic Bezier curve with easeOutExpo timing.
 * Returns a cancel function.
 */
export function flyTo(
  fromX: number,
  fromY: number,
  toX: number,
  toY: number,
  onUpdate: (x: number, y: number) => void,
  onComplete: () => void,
  duration = 600,
): () => void {
  const startTime = performance.now();
  // Control point: midpoint offset upward for arc effect
  const dist = Math.hypot(toX - fromX, toY - fromY);
  const arcHeight = Math.min(dist * 0.2, 80);
  const cpX = (fromX + toX) / 2;
  const cpY = (fromY + toY) / 2 - arcHeight;
  let cancelled = false;

  function easeOutExpo(t: number): number {
    return t >= 1 ? 1 : 1 - Math.pow(2, -10 * t);
  }

  function frame(now: number) {
    if (cancelled) return;
    const elapsed = now - startTime;
    const rawT = Math.min(elapsed / duration, 1);
    const t = easeOutExpo(rawT);

    // Quadratic Bezier: B(t) = (1-t)²P0 + 2(1-t)tP1 + t²P2
    const oneMinusT = 1 - t;
    const x = oneMinusT * oneMinusT * fromX + 2 * oneMinusT * t * cpX + t * t * toX;
    const y = oneMinusT * oneMinusT * fromY + 2 * oneMinusT * t * cpY + t * t * toY;

    onUpdate(x, y);

    if (rawT < 1) {
      requestAnimationFrame(frame);
    } else {
      onComplete();
    }
  }

  requestAnimationFrame(frame);

  return () => {
    cancelled = true;
  };
}
