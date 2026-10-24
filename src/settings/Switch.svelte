<script lang="ts">
  // An iOS-style switch. The track fades and the knob slides, so only opacity and transform animate.
  export let checked: boolean;
  export let label: string;
  export let disabled = false;
  export let onchange: (on: boolean) => void;
</script>

<button
  type="button"
  role="switch"
  class="switch"
  aria-checked={checked}
  aria-label={label}
  {disabled}
  onclick={() => onchange(!checked)}
>
  <span class="fill"></span>
  <span class="knob"></span>
</button>

<style>
  .switch {
    position: relative;
    flex: none;
    width: 51px;
    height: 31px;
    margin: 0;
    padding: 0;
    border: 0;
    border-radius: 16px;
    background: rgba(255, 255, 255, 0.18);
    cursor: pointer;
  }

  .switch:disabled {
    opacity: 0.4;
    cursor: default;
  }

  .switch:focus-visible {
    outline: 2px solid #0a84ff;
    outline-offset: 3px;
  }

  .fill {
    position: absolute;
    inset: 0;
    border-radius: inherit;
    background: #30d158;
    opacity: 0;
    transition: opacity 200ms ease;
  }

  /* The knob stays white; the track fades to green when on, as on iOS. */
  .knob {
    position: absolute;
    top: 2px;
    left: 2px;
    width: 27px;
    height: 27px;
    border-radius: 50%;
    background: #fff;
    box-shadow: 0 2px 6px rgba(0, 0, 0, 0.4);
    transition: transform var(--spring-bouncy-ms) var(--spring-bouncy);
  }

  .switch[aria-checked="true"] .fill {
    opacity: 1;
  }

  .switch[aria-checked="true"] .knob {
    transform: translateX(20px);
  }

  @media (prefers-reduced-motion: reduce) {
    .fill,
    .knob {
      transition: none;
    }
  }
</style>
