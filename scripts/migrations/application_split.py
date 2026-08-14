#!/usr/bin/env python3
"""Migrate the eight staging Clusters onto one Cluster + eight Applications.

The eight Komodo Clusters were one real cluster: same Server, same
kubeconfig, differing only in namespace and manifest directory. That was
forced by the old model, where a Cluster carried exactly one manifest
source. Now that manifests live on an Application, they collapse.

Steps, in order:

  1. create   - one Cluster (connection + policy) named by --cluster
  2. create   - one Application per snapshot entry, carrying its
                manifest half, with the environment prefix dropped
  3. diff     - DiffApplication each one
  4. delete   - the eight old Clusters, ONLY if every diff came back
                clean

Steps 1-3 are additive and reversible. Step 4 is not, which is why it
refuses to run unless step 3 proved the Application reproduces what the
Cluster was deploying. Deleting a Komodo Cluster never touches what is
running in Kubernetes, so even step 4 is not an outage - it is only
irreversible in Komodo's own bookkeeping.

Usage:
  application_split.py plan     # show what would happen, touch nothing
  application_split.py create   # steps 1-2
  application_split.py diff     # step 3, prints the gate verdict
  application_split.py delete   # step 4, refuses unless the gate passed
"""

import json
import os
import pathlib
import sys
import time
import urllib.error
import urllib.request

HOST = os.environ.get("KOMODO_HOST", "https://komodo.data.vici.corp")
KEY = os.environ.get("KOMODO_API_KEY")
SECRET = os.environ.get("KOMODO_API_SECRET")

HERE = pathlib.Path(__file__).parent
SNAPSHOT = HERE / "staging-clusters-snapshot.json"
# Written by `diff`, read by `delete`. The gate is a fact on disk rather
# than a promise in someone's head.
VERDICT = HERE / ".diff-verdict.json"

# The environment is the Cluster now, so repeating it in the Application
# name is the same duplication this whole change removes.
PREFIX = "staging-"


def call(group: str, kind: str, params: dict) -> dict:
    req = urllib.request.Request(
        f"{HOST}/{group}",
        data=json.dumps({"type": kind, "params": params}).encode(),
        headers={
            "Content-Type": "application/json",
            "X-API-KEY": KEY,
            "X-API-SECRET": SECRET,
        },
    )
    try:
        with urllib.request.urlopen(req, timeout=300) as res:
            return json.loads(res.read())
    except urllib.error.HTTPError as e:
        raise SystemExit(f"{kind} failed: {e.code} {e.read().decode()[:400]}")


def snapshot() -> list:
    if not SNAPSHOT.exists():
        raise SystemExit(f"missing {SNAPSHOT}; capture it before the new Core ships")
    return json.loads(SNAPSHOT.read_text())


def app_name(cluster_name: str) -> str:
    return cluster_name[len(PREFIX):] if cluster_name.startswith(PREFIX) else cluster_name


def cmd_plan(cluster: str) -> None:
    print(f"Cluster to create: {cluster}")
    for entry in snapshot():
        config = entry["config"]
        print(
            f"  {entry['name']:<24} -> Application {app_name(entry['name']):<16}"
            f" ns={config['namespace'] or '(cluster default)':<14}"
            f" dir={config['run_directory']}"
        )
    print(f"\n{len(snapshot())} Applications, then delete {len(snapshot())} Clusters once every diff is clean.")


def cmd_create(cluster: str) -> None:
    entries = snapshot()
    first = entries[0]["config"]

    existing = {c["name"] for c in call("read", "ListClusters", {})}
    if cluster in existing:
        print(f"Cluster {cluster} already exists, reusing it")
        cluster_id = next(
            c["id"] for c in call("read", "ListClusters", {}) if c["name"] == cluster
        )
    else:
        created = call(
            "write",
            "CreateCluster",
            {
                "name": cluster,
                "config": {
                    # Every one of the eight carried the same connection.
                    "server_id": first["server_id"],
                    "kubeconfig_path": first["kubeconfig_path"],
                    "context": first["context"],
                    "proxy_url": first["proxy_url"],
                    # Policy: unchanged from what the eight had, so the
                    # migration changes structure and nothing else.
                    "namespaces": first["namespaces"],
                    "cluster_resources": first["cluster_resources"],
                },
            },
        )
        cluster_id = created["_id"]["$oid"]
        print(f"created Cluster {cluster} ({cluster_id})")

    existing_apps = {a["name"] for a in call("read", "ListApplications", {})}
    for entry in entries:
        config = entry["config"]
        name = app_name(entry["name"])
        if name in existing_apps:
            print(f"  Application {name} already exists, skipping")
            continue
        call(
            "write",
            "CreateApplication",
            {
                "name": name,
                "config": {
                    "cluster_id": cluster_id,
                    "namespace": config["namespace"],
                    "linked_repo": config["linked_repo"],
                    "run_directory": config["run_directory"],
                    "file_paths": config["file_paths"],
                    "kustomize": config["kustomize"],
                    "wait_ready": config["wait_ready"],
                    "extra_args": config["extra_args"],
                    "file_contents": config["file_contents"],
                    "files_on_host": config["files_on_host"],
                    "skip_secret_interp": config["skip_secret_interp"],
                    "links": config["links"],
                },
            },
        )
        print(f"  created Application {name}")


def await_update(update_id: str) -> dict:
    for _ in range(240):
        update = call("read", "GetUpdate", {"id": update_id})
        if update["status"] == "Complete":
            return update
        time.sleep(2)
    raise SystemExit(f"update {update_id} never completed")


def cmd_diff() -> None:
    results = {}
    for entry in snapshot():
        name = app_name(entry["name"])
        print(f"diffing {name} ...", flush=True)
        update = call("execute", "DiffApplication", {"application": name})
        finished = await_update(update["_id"]["$oid"])
        state = call("read", "GetApplication", {"application": name})["info"]["state"]
        results[name] = {
            "success": finished["success"],
            "state": state,
            "update": update["_id"]["$oid"],
        }
        print(f"  success={finished['success']} state={state}")

    # Deployed means the diff ran and found nothing. Drifted means it
    # found something, which is a real answer and a blocking one here:
    # the Application is not reproducing what the Cluster deployed.
    clean = all(r["success"] and r["state"] == "Deployed" for r in results.values())
    VERDICT.write_text(json.dumps({"clean": clean, "results": results}, indent=2))

    print()
    if clean:
        print("GATE PASSED: every Application matches the cluster. Safe to delete the old Clusters.")
    else:
        bad = [n for n, r in results.items() if not (r["success"] and r["state"] == "Deployed")]
        print(f"GATE FAILED: {', '.join(bad)}")
        print("Do NOT delete the old Clusters. Fix the Application config until every diff is clean.")
        sys.exit(1)


def cmd_delete() -> None:
    if not VERDICT.exists():
        raise SystemExit("no diff verdict on disk - run `diff` first")
    verdict = json.loads(VERDICT.read_text())
    if not verdict.get("clean"):
        raise SystemExit("the last diff run did not pass the gate - refusing to delete")

    for entry in snapshot():
        call("write", "DeleteCluster", {"id": entry["name"]})
        print(f"deleted Cluster {entry['name']}")
    print("\nThe Kubernetes objects are untouched: deleting a Komodo Cluster removes only Komodo's record of it.")


def main() -> None:
    if not KEY or not SECRET:
        raise SystemExit("KOMODO_API_KEY / KOMODO_API_SECRET not set")
    cmd = sys.argv[1] if len(sys.argv) > 1 else "plan"
    cluster = os.environ.get("MIGRATION_CLUSTER", "staging")
    if cmd == "plan":
        cmd_plan(cluster)
    elif cmd == "create":
        cmd_create(cluster)
    elif cmd == "diff":
        cmd_diff()
    elif cmd == "delete":
        cmd_delete()
    else:
        raise SystemExit(__doc__)


if __name__ == "__main__":
    main()
