import { useVirtualizer } from "@tanstack/react-virtual";
import { useEffect, useMemo, useRef, useState } from "react";
import type { Game, GameSource, Settings, SortMode } from "../types";
import { IS_MACOS } from "../utils/platform";
import { GameCard } from "./GameCard";
import { MultiSelect, type MultiSelectOption } from "./MultiSelect";
import styles from "./LibrarySection.module.css";

interface LibrarySectionProps {
  games: Game[];
  onSelect: (game: Game) => void;
  /** `null` only for the brief moment before settings load from disk — the
   * defaults below match `Settings`' own Rust-side defaults for that gap. */
  settings: Settings | null;
  onUpdateSettings: (partial: Partial<Settings>) => void;
}

const sourceOptions: readonly MultiSelectOption<GameSource>[] = [
  { value: "steam", label: "Steam" },
  { value: "xbox", label: "Xbox" },
  { value: "battlenet", label: "Battle.net" },
  { value: "ea", label: "EA App" },
  { value: "retro", label: "Retro" },
].filter(
  (option): option is MultiSelectOption<GameSource> =>
    !IS_MACOS || (option.value !== "xbox" && option.value !== "battlenet" && option.value !== "ea"),
);

// Mirrors the grid's own `minmax(170px, 1fr)` track sizing so the
// JS-computed column count matches what the CSS would have laid out.
const CARD_MIN_WIDTH = 170;
const GAP_X = 18;
const GAP_Y = 22;
// A card's rendered height is fixed by its CSS (190px wide, 2:3 art +
// title) regardless of column width, so the row height is constant too.
const ROW_HEIGHT = 190 * 1.5 + 10 + 46 + GAP_Y;

function sortGames(games: Game[], mode: SortMode): Game[] {
  const sorted = [...games];
  switch (mode) {
    case "alphabetical":
      return sorted.sort((a, b) => a.name.localeCompare(b.name));
    case "playtime":
      return sorted.sort((a, b) => (b.playtimeMinutes ?? 0) - (a.playtimeMinutes ?? 0));
    case "score":
      return sorted.sort((a, b) => (b.igdbScore ?? 0) - (a.igdbScore ?? 0));
    case "completion":
      // -1 (not "unknown" 0) so a real 0%-complete game still outranks one
      // with no achievement data at all.
      return sorted.sort((a, b) => (b.achievementCompletionPercent ?? -1) - (a.achievementCompletionPercent ?? -1));
    case "recent":
    default:
      return sorted.sort((a, b) => (b.lastPlayed ?? 0) - (a.lastPlayed ?? 0));
  }
}

export function LibrarySection({ games, onSelect, settings, onUpdateSettings }: LibrarySectionProps) {
  const [search, setSearch] = useState(settings?.librarySearch ?? "");
  const [advancedOpen, setAdvancedOpen] = useState(false);
  const advancedRef = useRef<HTMLDivElement>(null);
  const sortMode: SortMode = settings?.librarySortMode ?? "playtime";
  const favoritesOnly = settings?.libraryFavoritesOnly ?? false;
  const showNotInstalled = settings?.libraryShowNotInstalled ?? false;
  const sourceFilter = (settings?.librarySourceFilter ?? []).filter((source) =>
    sourceOptions.some((option) => option.value === source),
  );
  const platformFilter = settings?.libraryPlatformFilter ?? [];
  const igdbGenreFilter = settings?.libraryIgdbGenreFilter ?? [];
  const igdbPlatformFilter = settings?.libraryIgdbPlatformFilter ?? [];
  const igdbDeveloperFilter = settings?.libraryIgdbDeveloperFilter ?? [];
  const igdbPublisherFilter = settings?.libraryIgdbPublisherFilter ?? [];
  const igdbMinScore = settings?.libraryIgdbMinScore ?? null;
  const igdbYearFrom = settings?.libraryIgdbYearFrom ?? null;
  const igdbYearTo = settings?.libraryIgdbYearTo ?? null;
  const igdbMetadataOnly = settings?.libraryIgdbMetadataOnly ?? false;

  const setSortMode = (mode: SortMode) => onUpdateSettings({ librarySortMode: mode });
  const setSearchValue = (value: string) => {
    setSearch(value);
    onUpdateSettings({ librarySearch: value });
  };
  const setFavoritesOnly = (value: boolean) => onUpdateSettings({ libraryFavoritesOnly: value });
  const setSourceFilter = (value: GameSource[]) => onUpdateSettings({ librarySourceFilter: value });
  const setPlatformFilter = (value: string[]) => onUpdateSettings({ libraryPlatformFilter: value });
  const setIgdbGenreFilter = (value: string[]) => onUpdateSettings({ libraryIgdbGenreFilter: value });
  const setIgdbPlatformFilter = (value: string[]) => onUpdateSettings({ libraryIgdbPlatformFilter: value });
  const setIgdbDeveloperFilter = (value: string[]) => onUpdateSettings({ libraryIgdbDeveloperFilter: value });
  const setIgdbPublisherFilter = (value: string[]) => onUpdateSettings({ libraryIgdbPublisherFilter: value });

  const platforms = useMemo(() => {
    const set = new Set<string>();
    for (const g of games) {
      if (g.platform) set.add(g.platform);
    }
    return Array.from(set).sort();
  }, [games]);

  const metadataOptions = useMemo(() => {
    const genres = new Set<string>();
    const platforms = new Set<string>();
    const developers = new Set<string>();
    const publishers = new Set<string>();
    for (const game of games) {
      for (const genre of game.igdbMetadata?.genres ?? []) genres.add(genre);
      for (const platform of game.igdbMetadata?.platforms ?? []) platforms.add(platform);
      for (const developer of game.igdbMetadata?.developers ?? []) developers.add(developer);
      for (const publisher of game.igdbMetadata?.publishers ?? []) publishers.add(publisher);
    }
    return {
      genres: Array.from(genres).sort((a, b) => a.localeCompare(b)),
      platforms: Array.from(platforms).sort((a, b) => a.localeCompare(b)),
      developers: Array.from(developers).sort((a, b) => a.localeCompare(b)),
      publishers: Array.from(publishers).sort((a, b) => a.localeCompare(b)),
    };
  }, [games]);

  useEffect(() => {
    if (settings) setSearch(settings.librarySearch);
  }, [settings?.librarySearch]);

  useEffect(() => {
    if (!advancedOpen) return;

    function handlePointerDown(event: PointerEvent) {
      if (!advancedRef.current?.contains(event.target as Node)) setAdvancedOpen(false);
    }

    function handleKeyDown(event: KeyboardEvent) {
      if (event.key !== "Escape") return;
      event.preventDefault();
      event.stopPropagation();
      setAdvancedOpen(false);
    }

    window.addEventListener("pointerdown", handlePointerDown);
    window.addEventListener("keydown", handleKeyDown, true);
    return () => {
      window.removeEventListener("pointerdown", handlePointerDown);
      window.removeEventListener("keydown", handleKeyDown, true);
    };
  }, [advancedOpen]);

  // Drop platform filters that no longer apply after a ROM directory or
  // rescan changes the available platform list.
  useEffect(() => {
    const valid = platformFilter.filter((platform) => platforms.includes(platform));
    if (valid.length !== platformFilter.length) {
      setPlatformFilter(valid);
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [platforms, platformFilter]);

  const filtered = useMemo(() => {
    let list = games;
    if (sourceFilter.length > 0) list = list.filter((g) => sourceFilter.includes(g.source));
    if (platformFilter.length > 0) {
      list = list.filter((g) => g.platform != null && platformFilter.includes(g.platform));
    }
    if (!showNotInstalled) list = list.filter((g) => g.isInstalled);
    if (igdbGenreFilter.length > 0) {
      list = list.filter((g) => g.igdbMetadata?.genres.some((genre) => igdbGenreFilter.includes(genre)));
    }
    if (igdbPlatformFilter.length > 0) {
      list = list.filter((g) => g.igdbMetadata?.platforms.some((platform) => igdbPlatformFilter.includes(platform)));
    }
    if (igdbDeveloperFilter.length > 0) {
      list = list.filter((g) => g.igdbMetadata?.developers.some((developer) => igdbDeveloperFilter.includes(developer)));
    }
    if (igdbPublisherFilter.length > 0) {
      list = list.filter((g) => g.igdbMetadata?.publishers.some((publisher) => igdbPublisherFilter.includes(publisher)));
    }
    if (igdbMinScore != null) {
      list = list.filter((g) => g.igdbScore != null && g.igdbScore >= igdbMinScore);
    }
    if (igdbYearFrom != null || igdbYearTo != null) {
      list = list.filter((g) => {
        const timestamp = g.igdbMetadata?.firstReleaseDate;
        if (!timestamp) return false;
        const year = new Date(timestamp * 1000).getUTCFullYear();
        return (igdbYearFrom == null || year >= igdbYearFrom) && (igdbYearTo == null || year <= igdbYearTo);
      });
    }
    if (igdbMetadataOnly) list = list.filter((g) => g.igdbMetadata != null);
    if (favoritesOnly) list = list.filter((g) => g.isFavorite);
    if (search.trim()) {
      const q = search.trim().toLowerCase();
      list = list.filter((g) => g.name.toLowerCase().includes(q));
    }
    return sortGames(list, sortMode);
  }, [
    games,
    search,
    sortMode,
    favoritesOnly,
    showNotInstalled,
    sourceFilter,
    platformFilter,
    igdbGenreFilter,
    igdbPlatformFilter,
    igdbDeveloperFilter,
    igdbPublisherFilter,
    igdbMinScore,
    igdbYearFrom,
    igdbYearTo,
    igdbMetadataOnly,
  ]);

  const advancedFilterCount = [
    igdbGenreFilter.length > 0,
    igdbPlatformFilter.length > 0,
    igdbDeveloperFilter.length > 0,
    igdbPublisherFilter.length > 0,
    igdbMinScore != null,
    igdbYearFrom != null || igdbYearTo != null,
    igdbMetadataOnly,
    showNotInstalled,
  ].filter(Boolean).length;

  function clearAdvancedFilters() {
    onUpdateSettings({
      libraryIgdbGenreFilter: [],
      libraryIgdbPlatformFilter: [],
      libraryIgdbDeveloperFilter: [],
      libraryIgdbPublisherFilter: [],
      libraryIgdbMinScore: null,
      libraryIgdbYearFrom: null,
      libraryIgdbYearTo: null,
      libraryIgdbMetadataOnly: false,
      libraryShowNotInstalled: false,
    });
  }

  const scrollRef = useRef<HTMLDivElement>(null);
  const [containerWidth, setContainerWidth] = useState(0);

  useEffect(() => {
    const el = scrollRef.current;
    if (!el) return;
    const observer = new ResizeObserver((entries) => {
      setContainerWidth(entries[0].contentRect.width);
    });
    observer.observe(el);
    return () => observer.disconnect();
  }, []);

  const columns = Math.max(1, Math.floor((containerWidth + GAP_X) / (CARD_MIN_WIDTH + GAP_X)));
  const rowCount = Math.ceil(filtered.length / columns);

  const virtualizer = useVirtualizer({
    count: rowCount,
    getScrollElement: () => scrollRef.current,
    estimateSize: () => ROW_HEIGHT,
    overscan: 3,
  });

  return (
    <section className={styles.section}>
      <div className={styles.sectionHeader}>
        <div className={styles.headingGroup}>
          <h2 className={styles.heading}>Library</h2>
          <span className={styles.count}>{filtered.length}</span>
        </div>
        <div className={styles.controls}>
          <input
            type="text"
            className={styles.searchInput}
            placeholder="Search games…"
            value={search}
            onChange={(e) => setSearchValue(e.target.value)}
            data-nav-item
          />
          <MultiSelect
            options={sourceOptions}
            selected={sourceFilter}
            onChange={setSourceFilter}
            allLabel="All sources"
            selectedLabel={(count) => `${count} sources`}
            ariaLabel="Filter by source"
          />
          {platforms.length > 0 && (
            <MultiSelect
              options={platforms.map((platform) => ({ value: platform, label: platform }))}
              selected={platformFilter}
              onChange={setPlatformFilter}
              allLabel="All platforms"
              selectedLabel={(count) => `${count} platforms`}
              ariaLabel="Filter by platform"
            />
          )}
          <div
            className={styles.advancedFilterWrapper}
            ref={advancedRef}
            onBlur={(event) => {
              if (event.relatedTarget && !advancedRef.current?.contains(event.relatedTarget)) {
                setAdvancedOpen(false);
              }
            }}
          >
            <button
              type="button"
              className={`${styles.advancedToggle} ${advancedFilterCount > 0 ? styles.active : ""}`}
              data-nav-item
              onClick={() => setAdvancedOpen((value) => !value)}
              aria-expanded={advancedOpen}
              aria-haspopup="dialog"
            >
              Filters{advancedFilterCount > 0 ? ` · ${advancedFilterCount}` : ""}
            </button>
            {advancedOpen && (
              <div className={styles.advancedPanel} role="dialog" aria-label="Advanced filters">
                <div className={styles.advancedHeader}>
                  <strong>Filters</strong>
                  <button
                    type="button"
                    className={styles.advancedClose}
                    data-nav-item
                    onClick={() => setAdvancedOpen(false)}
                    aria-label="Close advanced filters"
                  >
                    ×
                  </button>
                </div>
                <div className={styles.advancedGrid}>
                  {metadataOptions.genres.length > 0 && (
                    <MultiSelect
                      options={metadataOptions.genres.map((value) => ({ value, label: value }))}
                      selected={igdbGenreFilter}
                      onChange={setIgdbGenreFilter}
                      allLabel="All genres"
                      selectedLabel={(count) => `${count} genres`}
                      ariaLabel="Filter by IGDB genre"
                    />
                  )}
                  {metadataOptions.platforms.length > 0 && (
                    <MultiSelect
                      options={metadataOptions.platforms.map((value) => ({ value, label: value }))}
                      selected={igdbPlatformFilter}
                      onChange={setIgdbPlatformFilter}
                      allLabel="All IGDB platforms"
                      selectedLabel={(count) => `${count} IGDB platforms`}
                      ariaLabel="Filter by IGDB platform"
                    />
                  )}
                  {metadataOptions.developers.length > 0 && (
                    <MultiSelect
                      options={metadataOptions.developers.map((value) => ({ value, label: value }))}
                      selected={igdbDeveloperFilter}
                      onChange={setIgdbDeveloperFilter}
                      allLabel="All developers"
                      selectedLabel={(count) => `${count} developers`}
                      ariaLabel="Filter by IGDB developer"
                    />
                  )}
                  {metadataOptions.publishers.length > 0 && (
                    <MultiSelect
                      options={metadataOptions.publishers.map((value) => ({ value, label: value }))}
                      selected={igdbPublisherFilter}
                      onChange={setIgdbPublisherFilter}
                      allLabel="All publishers"
                      selectedLabel={(count) => `${count} publishers`}
                      ariaLabel="Filter by IGDB publisher"
                    />
                  )}
                  <label className={styles.advancedField}>
                    <span>Minimum score</span>
                    <select
                      className={styles.advancedSelect}
                      value={igdbMinScore ?? ""}
                      onChange={(event) =>
                        onUpdateSettings({
                          libraryIgdbMinScore: event.target.value ? Number(event.target.value) : null,
                        })
                      }
                      data-nav-item
                    >
                      <option value="">Any score</option>
                      <option value="60">60+</option>
                      <option value="70">70+</option>
                      <option value="80">80+</option>
                      <option value="90">90+</option>
                    </select>
                  </label>
                  <div className={styles.advancedField}>
                    <span>Release year</span>
                    <div className={styles.yearInputs}>
                      <input
                        type="number"
                        className={styles.yearInput}
                        placeholder="From"
                        min="1970"
                        max="2100"
                        value={igdbYearFrom ?? ""}
                        onChange={(event) =>
                          onUpdateSettings({
                            libraryIgdbYearFrom: event.target.value ? Number(event.target.value) : null,
                          })
                        }
                        data-nav-item
                        aria-label="Release year from"
                      />
                      <input
                        type="number"
                        className={styles.yearInput}
                        placeholder="To"
                        min="1970"
                        max="2100"
                        value={igdbYearTo ?? ""}
                        onChange={(event) =>
                          onUpdateSettings({
                            libraryIgdbYearTo: event.target.value ? Number(event.target.value) : null,
                          })
                        }
                        data-nav-item
                        aria-label="Release year to"
                      />
                    </div>
                  </div>
                </div>
                <label className={styles.metadataOnly}>
                  <input
                    type="checkbox"
                    checked={igdbMetadataOnly}
                    onChange={(event) => onUpdateSettings({ libraryIgdbMetadataOnly: event.target.checked })}
                    data-nav-item
                  />
                  <span>Only games with IGDB metadata</span>
                </label>
                <label className={styles.metadataOnly}>
                  <input
                    type="checkbox"
                    checked={showNotInstalled}
                    onChange={(event) => onUpdateSettings({ libraryShowNotInstalled: event.target.checked })}
                    data-nav-item
                  />
                  <span>Show not installed games</span>
                </label>
                {advancedFilterCount > 0 && (
                  <button
                    type="button"
                    className={styles.clearAdvanced}
                    onClick={clearAdvancedFilters}
                    data-nav-item
                  >
                    Clear filters
                  </button>
                )}
              </div>
            )}
          </div>
          <select
            className={styles.sortSelect}
            value={sortMode}
            onChange={(e) => setSortMode(e.target.value as SortMode)}
            data-nav-item
            aria-label="Sort games"
          >
            <option value="alphabetical">A–Z</option>
            <option value="recent">Recently played</option>
            <option value="playtime">Playtime</option>
            <option value="score">IGDB score</option>
            <option value="completion">Completion %</option>
          </select>
          <button
            type="button"
            className={`${styles.favoritesToggle} ${favoritesOnly ? styles.active : ""}`}
            onClick={() => setFavoritesOnly(!favoritesOnly)}
            data-nav-item
            aria-pressed={favoritesOnly}
          >
            ★ Favorites
          </button>
        </div>
      </div>

      {/* Always mounted, even with zero results — this is what
          `ResizeObserver` is attached to, and it must never unmount while
          the section itself is still on screen. Swapping it out for a
          plain <p> when filters produced no matches (as this used to do)
          orphaned the observer on the next filter change, permanently
          stuck showing a single column. */}
      <div ref={scrollRef} className={`${styles.scrollArea} wervik-scroll`}>
        {filtered.length === 0 ? (
          <p className={styles.emptyFilter}>No games match your filters.</p>
        ) : (
          <div style={{ height: virtualizer.getTotalSize(), position: "relative" }}>
            {virtualizer.getVirtualItems().map((virtualRow) => {
              const start = virtualRow.index * columns;
              const rowGames = filtered.slice(start, start + columns);
              return (
                <div
                  key={virtualRow.key}
                  ref={virtualizer.measureElement}
                  data-index={virtualRow.index}
                  className={styles.row}
                  style={{
                    position: "absolute",
                    top: 0,
                    left: 0,
                    width: "100%",
                    transform: `translateY(${virtualRow.start}px)`,
                    gridTemplateColumns: `repeat(${columns}, minmax(${CARD_MIN_WIDTH}px, 1fr))`,
                  }}
                >
                  {rowGames.map((game) => (
                    <GameCard key={game.id} game={game} onSelect={onSelect} />
                  ))}
                </div>
              );
            })}
          </div>
        )}
      </div>
    </section>
  );
}
