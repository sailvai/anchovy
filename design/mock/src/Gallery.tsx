import { screens, themes } from "./catalog";

// Every screen, with a link to each theme.
export function Gallery() {
  return (
    <div className="mx-auto max-w-[640px] px-6 py-10">
      <h1 className="text-[20px] font-semibold">Anchovy mock</h1>
      <p className="mt-1 text-muted">
        Static screens with fake data, for plan step 2a. Nothing here calls Rust.
      </p>
      <ul className="mt-6 divide-y divide-line rounded-lg border border-line">
        {screens.map((screen) => (
          <li key={screen.id} className="flex items-center gap-3 px-4 py-2">
            <span className="flex-1">{screen.title}</span>
            {themes.map((option) => (
              <a
                key={option}
                href={`?screen=${screen.id}&theme=${option}`}
                className="rounded px-2 py-0.5 text-[12px] text-muted hover:bg-hover hover:text-text"
              >
                {option === "light" ? "Light" : "Dark"}
              </a>
            ))}
          </li>
        ))}
      </ul>
    </div>
  );
}
