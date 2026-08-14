import { ICONS } from "@/lib/icons";
import { usableResourcePath } from "@/lib/utils";
import {
  SIDEBAR_GROUPS,
  UNGROUPED_SIDEBAR_RESOURCES,
  UsableResource,
} from "@/resources";
import {
  Button,
  Divider,
  ScrollArea,
  Stack,
  Text,
  Tooltip,
} from "@mantine/core";
import { Fragment, ReactNode } from "react";
import { Link, useLocation } from "react-router-dom";

/// Resource types whose plural is not just "+s".
const SIDEBAR_LABELS: Partial<Record<UsableResource, string>> = {
  ResourceSync: "Syncs",
  Terraform: "Terraform",
};

const Sidebar = ({
  close,
  collapsed = false,
}: {
  close: () => void;
  collapsed?: boolean;
}) => {
  const location = useLocation().pathname;
  const linkProps = { location, close, collapsed };
  return (
    <Stack
      justify="space-between"
      gap="md"
      h="96%"
      m={collapsed ? "xs" : "xl"}
      mt="24"
      mr={collapsed ? "xs" : "md"}
    >
      {/* TOP AREA (scrolling) */}
      <ScrollArea>
        <Stack gap="0.15rem" mr={collapsed ? "0" : "md"}>
          <SidebarLink
            label="Dashboard"
            icon={<ICONS.Dashboard size="1rem" />}
            to="/"
            {...linkProps}
          />
          <SidebarLink
            label="Containers"
            icon={<ICONS.Container size="1rem" />}
            to="/containers"
            {...linkProps}
          />
          <SidebarLink
            label="Terminals"
            icon={<ICONS.Terminal size="1rem" />}
            to="/terminals"
            {...linkProps}
          />
          <SidebarLink
            label="Stats"
            icon={<ICONS.Stats size="1rem" />}
            to="/stats"
            {...linkProps}
          />

          {[
            ...SIDEBAR_GROUPS,
            ...(UNGROUPED_SIDEBAR_RESOURCES.length
              ? [
                  {
                    label: "Other",
                    resources: UNGROUPED_SIDEBAR_RESOURCES,
                  },
                ]
              : []),
          ].map((group) => (
            <Fragment key={group.label}>
              <SidebarDivider label={group.label} collapsed={collapsed} />
              {[
                ...group.resources.map((type) => ({ type, nested: false })),
                ...(group.nested ?? []).map((type) => ({
                  type,
                  nested: true,
                })),
              ].map(({ type, nested }) => {
                const Icon = ICONS[type];
                return (
                  <SidebarLink
                    key={type}
                    // Terraform is a mass noun, like the Syncs special
                    // case above: "Terraforms" reads as a verb.
                    label={SIDEBAR_LABELS[type] ?? type + "s"}
                    icon={<Icon size="1rem" />}
                    to={`/${usableResourcePath(type)}`}
                    nested={nested}
                    {...linkProps}
                  />
                );
              })}
            </Fragment>
          ))}

          <SidebarDivider label="Notifications" collapsed={collapsed} />

          <SidebarLink
            label="Alerts"
            icon={<ICONS.Alert size="1rem" />}
            to="/alerts"
            {...linkProps}
          />
          <SidebarLink
            label="Updates"
            icon={<ICONS.Update size="1rem" />}
            to="/updates"
            {...linkProps}
          />

          <Divider my="xs" />

          <SidebarLink
            label="Schedules"
            icon={<ICONS.Schedule size="1rem" />}
            to="/schedules"
            {...linkProps}
          />
          <SidebarLink
            label="Settings"
            icon={<ICONS.Settings size="1rem" />}
            to="/settings"
            {...linkProps}
          />
        </Stack>
      </ScrollArea>

      {/* BOTTOM AREA */}
      <Stack gap="lg" />
    </Stack>
  );
};

/// Collapsed, the section label has nowhere to go without wrapping, so
/// the divider degrades to a plain rule rather than being dropped —
/// the grouping is still worth showing.
const SidebarDivider = ({
  label,
  collapsed,
}: {
  label: string;
  collapsed: boolean;
}) =>
  collapsed ? (
    <Divider my="0.35rem" />
  ) : (
    <Divider
      label={
        <Text opacity={0.7} size="sm">
          {label}
        </Text>
      }
      my="0.1rem"
    />
  );

const SidebarLink = ({
  label,
  icon,
  to,
  location,
  close,
  collapsed,
  nested,
}: {
  label: string;
  icon: ReactNode;
  to: string;
  location: string;
  close: () => void;
  collapsed: boolean;
  /** Renders indented, to show this resource belongs to the one above. */
  nested?: boolean;
}) => {
  const active = to === "/" ? location === "/" : location.startsWith(to);
  const button = (
    <Button
      variant={active ? "default" : "subtle"}
      component={Link}
      to={to}
      onClick={close}
      // Collapsed, the icon becomes the whole target, so it is centred
      // and the label is dropped rather than clipped.
      leftSection={collapsed ? undefined : icon}
      justify={collapsed ? "center" : "flex-start"}
      // Collapsed the rail is icons only, so an indent would just eat
      // the target; the nesting is carried by the tooltip's label.
      px={collapsed ? "0" : undefined}
      ml={!collapsed && nested ? "md" : undefined}
      w={!collapsed && nested ? "calc(100% - var(--mantine-spacing-md))" : undefined}
      fullWidth
      aria-label={collapsed ? label : undefined}
    >
      {collapsed ? icon : label}
    </Button>
  );
  // The tooltip is the only thing that names a collapsed icon, so it is
  // the label rather than a decoration. Mantine opens it on focus too,
  // which keeps the rail usable from the keyboard.
  return collapsed ? (
    <Tooltip label={label} position="right" withArrow openDelay={200}>
      {button}
    </Tooltip>
  ) : (
    button
  );
};

export default Sidebar;
