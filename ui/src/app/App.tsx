import { lazy, Suspense } from "react";
import { Toaster } from "sonner";
import { TipProvider } from "../components/ui";
import { CommandProvider } from "./commands";
import { useRoute } from "./router";
import { StatusProvider } from "./status";
import { ThemeProvider, useTheme } from "./theme";
import { Workbench } from "./Workbench";

const Landing = lazy(() => import("../features/landing/Landing"));

function Toasts() {
  const { dark } = useTheme();
  return <Toaster theme={dark ? "dark" : "light"} position="bottom-right" richColors closeButton />;
}

export function App() {
  const [route] = useRoute();
  return (
    <ThemeProvider>
      <TipProvider delay={400}>
        <CommandProvider>
          <StatusProvider>
            {route === "landing" ? (
              <Suspense fallback={null}>
                <Landing />
              </Suspense>
            ) : (
              <Workbench workspace={route} />
            )}
            <Toasts />
          </StatusProvider>
        </CommandProvider>
      </TipProvider>
    </ThemeProvider>
  );
}
