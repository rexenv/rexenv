import { useQuery } from "@tanstack/react-query";
import { getPlatformWords } from "@/lib/ipc";
import { mockPlatformWords } from "@/lib/mock";
import type { PlatformWords } from "@/types";

/** The words for this OS's own things ("Show in Finder" / "Show in Explorer", the login item's description),
 *  from the backend — the platform owns them (W7 S7, ruling Q3). Static per build, so read once. Until the
 *  first answer arrives, macOS's words: what the app always showed, and what a macOS build answers anyway. */
export function usePlatformWords(): PlatformWords {
  const { data } = useQuery({
    queryKey: ["platform-words"],
    queryFn: getPlatformWords,
    staleTime: Infinity,
  });
  return data ?? mockPlatformWords;
}
