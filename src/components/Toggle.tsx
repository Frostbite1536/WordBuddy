export function Toggle({
  checked,
  onChange,
  disabled,
  label,
}: {
  checked: boolean;
  onChange: () => void;
  disabled?: boolean;
  label: string;
}) {
  return (
    <button
      role="switch"
      aria-checked={checked}
      aria-label={label}
      onClick={onChange}
      disabled={disabled}
      className={`w-10 h-5 rounded-full transition-colors disabled:opacity-30 ${
        checked ? "bg-accent" : "bg-zinc-700"
      }`}
    >
      <div
        className={`w-4 h-4 bg-white rounded-full transition-transform mx-0.5 ${
          checked ? "translate-x-5" : ""
        }`}
      />
    </button>
  );
}
