import { AppShell, Box } from "@mantine/core";
import { useDisclosure, useLocalStorage } from "@mantine/hooks";
import { Suspense } from "react";
import { Outlet } from "react-router-dom";
import Topbar from "@/app/topbar";
import Sidebar from "@/app/sidebar";
import { LoadingScreen } from "mogh_ui";
import UpdateDetails from "@/components/updates/details";
import AlertDetails from "@/components/alerts/details";

export const TOPBAR_HEIGHT = 62;

export const SIDEBAR_WIDTH = 240;
/// Collapsed, the navbar narrows to a rail instead of using AppShell's
/// `collapsed.desktop` — that hides it outright, and the point of
/// collapsing here is to keep the icons reachable.
export const SIDEBAR_COLLAPSED_WIDTH = 62;

const App = () => {
  const [opened, { toggle, close }] = useDisclosure();
  // Persisted: a layout preference that resets on every reload is worse
  // than not having one.
  const [collapsed, setCollapsed] = useLocalStorage<boolean>({
    key: "sidebar-collapsed-v1",
    defaultValue: false,
  });
  return (
    <AppShell
      padding={{ base: "lg", sm: "xl" }}
      header={{ height: TOPBAR_HEIGHT }}
      navbar={{
        width: collapsed ? SIDEBAR_COLLAPSED_WIDTH : SIDEBAR_WIDTH,
        breakpoint: "sm",
        // Mobile still opens the full-width drawer, so the rail is a
        // desktop-only concept.
        collapsed: { mobile: !opened },
      }}
    >
      <Topbar
        opened={opened}
        toggle={toggle}
        collapsed={collapsed}
        toggleCollapsed={() => setCollapsed((c) => !c)}
      />

      <AppShell.Navbar
        style={(theme) => {
          return {
            borderColor: theme.colors["accent-border"][1],
          };
        }}
      >
        <Sidebar close={close} collapsed={collapsed} />
      </AppShell.Navbar>

      <AppShell.Main>
        <Suspense fallback={<LoadingScreen />}>
          <Box px={{ xl: "xl" }}>
            <Outlet />
          </Box>
          <UpdateDetails />
          <AlertDetails />
        </Suspense>
      </AppShell.Main>
    </AppShell>
  );
};

export default App;
