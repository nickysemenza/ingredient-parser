import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { WasmContextProvider } from "./WasmProvider";
import { Demo } from "./Demo";

// Scraped recipes don't change underneath us, so cache them indefinitely —
// re-pasting a recent URL is served from cache rather than re-fetched.
const queryClient = new QueryClient({
  defaultOptions: {
    queries: { staleTime: Infinity, retry: 1, refetchOnWindowFocus: false },
  },
});

function App() {
  return (
    <QueryClientProvider client={queryClient}>
      <WasmContextProvider>
        <Demo />
      </WasmContextProvider>
    </QueryClientProvider>
  );
}

export default App;
