import { TextInput } from "@mantine/core";
import { useCallback, useMemo, useState } from "react";

/// Per-column text filtering for a DataTable.
///
/// DataTable itself has no column-filter support (it lives in mogh_ui),
/// so this narrows the row array before handing it over rather than
/// filtering inside the table. Two consequences worth knowing:
///
/// - It filters what the table was given. On a server-side-paginated
///   table that is the current page, not the whole collection.
/// - Column ids here are ours, not the table's; they only have to match
///   between the input and the extractor map below.
export function useFieldFilters<T>(
  extractors: Record<string, (row: T) => string | undefined | null>,
) {
  const [filters, setFilters] = useState<Record<string, string>>({});

  const setFilter = useCallback((id: string, value: string) => {
    setFilters((current) => ({ ...current, [id]: value }));
  }, []);

  const filterRows = useCallback(
    (rows: T[]) => {
      const active = Object.entries(filters).filter(([, term]) => term);
      if (!active.length) return rows;
      return rows.filter((row) =>
        active.every(([id, term]) => {
          const value = extractors[id]?.(row) ?? "";
          return value.toLowerCase().includes(term.toLowerCase());
        }),
      );
    },
    // extractors is written inline at the call site, so depending on the
    // object itself would rebuild this every render. The keys are what
    // actually change.
    [filters, Object.keys(extractors).join()],
  );

  const anyActive = useMemo(
    () => Object.values(filters).some((term) => !!term),
    [filters],
  );

  return { filters, setFilter, filterRows, anyActive };
}

/// The input rendered under a column header.
///
/// Clicks are stopped so typing in it never triggers the header's sort.
export function FieldFilter({
  id,
  filters,
  setFilter,
  placeholder,
}: {
  id: string;
  filters: Record<string, string>;
  setFilter: (id: string, value: string) => void;
  placeholder?: string;
}) {
  return (
    <TextInput
      size="xs"
      mt={4}
      value={filters[id] ?? ""}
      placeholder={placeholder ?? "Filter"}
      onClick={(e) => e.stopPropagation()}
      onChange={(e) => setFilter(id, e.currentTarget.value)}
    />
  );
}
