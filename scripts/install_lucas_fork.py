#!/usr/bin/env python3
"""Install a validated Jcode fork artifact, preserving local config and rules.

No credentials/config/session files are transferred. Run on each target user.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import time
from datetime import datetime
from zoneinfo import ZoneInfo


def require(condition, message):
    if not condition:
        raise RuntimeError(message)


def digest(path):
    with path.open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def atomic_link(target, link):
    link.parent.mkdir(parents=True, exist_ok=True)
    tmp = link.with_name(link.name + '.fork-install-tmp')
    if tmp.exists() or tmp.is_symlink():
        raise RuntimeError('Temporary link already exists: ' + str(tmp))
    tmp.symlink_to(target)
    tmp.replace(link)


def protected(home):
    paths = [home / 'AGENTS.md', home / 'CLAUDE.md']
    paths += [home / '.jcode' / name for name in (
        'config.toml', 'mcp.json', 'AGENTS.md', 'CLAUDE.md', 'system_prompt.md')]
    rules = home / '.jcode/rules'
    if rules.is_dir():
        paths += [p for p in rules.rglob('*') if p.is_file()]
    return {str(p.relative_to(home)): digest(p) for p in paths if p.is_file()}


def daemons():
    found = []
    for proc in Path('/proc').iterdir():
        if not proc.name.isdigit():
            continue
        try:
            if proc.stat().st_uid != os.getuid():
                continue
            argv = (proc / 'cmdline').read_bytes().split(b'\0')
            if b'serve' in argv and 'jcode' in str((proc / 'exe').resolve()):
                found.append({'pid': int(proc.name), 'exe': str((proc / 'exe').resolve())})
        except (PermissionError, FileNotFoundError, ProcessLookupError):
            pass
    return found


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('binary', type=Path)
    parser.add_argument('--sha256', required=True)
    parser.add_argument('--source-commit', required=True)
    parser.add_argument('--label', required=True)
    parser.add_argument('--reload', action='store_true')
    parser.add_argument('--apply', action='store_true')
    args = parser.parse_args()
    require(re.fullmatch('[a-zA-Z0-9._-]+', args.label), 'Unsafe version label')
    require(args.label not in ('.', '..'), 'Unsafe version label')
    require(re.fullmatch(r'[0-9a-f]{7,40}', args.source_commit), 'Invalid source commit')
    binary = args.binary.resolve()
    require(digest(binary) == args.sha256, 'Artifact checksum mismatch')
    # Help/version are offline and do not initialize providers or edit config.
    version = subprocess.check_output([str(binary), '--version'], text=True).strip()
    require(args.source_commit[:9] in version, 'Build source identity mismatch')
    home = Path.home()
    require(not os.environ.get('JCODE_HOME') or Path(os.environ['JCODE_HOME']).resolve() == home / '.jcode',
            'Install from the standard user JCODE_HOME, outside isolated test environments')
    store = home / '.jcode/builds'
    launcher = home / '.local/bin/jcode'
    hashes = protected(home)
    before_daemons = daemons()
    old_wrapper = launcher.resolve().read_bytes()[:4096] if launcher.is_file() else b''
    disable_claude = b'JCODE_DISABLE_CLAUDE_MCP' in old_wrapper
    if not args.apply:
        print(json.dumps({'dry_run': True, 'label': args.label, 'version': version,
                          'protected_files': len(hashes), 'daemons': before_daemons,
                          'preserve_claude_mcp_exclusion': disable_claude}))
        return
    os.umask(0o077)
    report = home / '.local/state/jcode-fork' / args.label / datetime.now(ZoneInfo('America/Sao_Paulo')).strftime('%Y%m%d-%H%M%S')
    report.mkdir(parents=True, exist_ok=False)
    links = [launcher] + [store / channel / 'jcode' for channel in ('current', 'stable', 'shared-server')]
    markers = [store / (channel + '-version') for channel in ('current', 'stable', 'shared-server')]
    before = {'links': {}, 'markers': {}, 'protected_sha256': hashes, 'daemons': before_daemons}
    for link in links:
        if link.is_symlink():
            before['links'][str(link)] = os.readlink(link)
        elif link.exists():
            raise RuntimeError('Refusing to replace non-symlink launcher/channel: ' + str(link))
        else:
            before['links'][str(link)] = None
    for marker in markers:
        before['markers'][str(marker)] = marker.read_text() if marker.is_file() else None
    (report / 'before.json').write_text(json.dumps(before, indent=2) + '\n')
    for relative in hashes:
        src = home / relative
        dst = report / 'protected' / relative
        dst.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(src, dst)
        dst.chmod(0o600)
    dest = store / 'versions' / args.label
    dest.mkdir(parents=True, exist_ok=True)
    installed_binary = dest / 'jcode-linux-x86_64.bin'
    if installed_binary.exists():
        require(digest(installed_binary) == args.sha256, 'Immutable version already has another binary')
    else:
        shutil.copyfile(binary, installed_binary)
        installed_binary.chmod(0o755)
    wrapper = '''#!/usr/bin/env sh
set -eu
# Lucas fork: retain project resume and avoid official updates replacing patches.
export JCODE_RESUME_PROJECT_SCOPE=1
export JCODE_CHECK_UPDATES=0
'''
    if disable_claude:
        wrapper += 'export JCODE_DISABLE_CLAUDE_MCP=1\n'
    wrapper += '''self_path=$(readlink -f -- "$0")
self_dir=$(dirname -- "$self_path")
export LD_LIBRARY_PATH="$self_dir${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
exec "$self_dir/jcode-linux-x86_64.bin" "$@"
'''
    installed_wrapper = dest / 'jcode'
    if installed_wrapper.exists():
        require(installed_wrapper.read_text() == wrapper, 'Immutable version wrapper differs')
    else:
        installed_wrapper.write_text(wrapper)
        installed_wrapper.chmod(0o755)
    # Upstream migrations must not overwrite this user's deliberate choices.
    created_migrations = []
    for name in ('swarm-spawn-mode-inline', 'idle-animation-off'):
        migration = home / '.jcode/migrations' / name
        if not migration.exists():
            migration.parent.mkdir(parents=True, exist_ok=True)
            migration.write_text('Lucas fork update: existing user preference preserved.\n')
            created_migrations.append(str(migration))
    before['created_migrations'] = created_migrations
    (report / 'before.json').write_text(json.dumps(before, indent=2) + '\n')
    for channel in ('current', 'stable'):
        atomic_link(installed_wrapper, store / channel / 'jcode')
        (store / (channel + '-version')).write_text(args.label + '\n')
    atomic_link(store / 'current/jcode', launcher)
    env = dict(os.environ, JCODE_CHECK_UPDATES='0')
    def cli(command, timeout=70):
        result = subprocess.run([str(launcher), '--no-update', *command],
                                stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                                text=True, timeout=timeout, env=env)
        # Persist command responses locally. Never include config or model secrets.
        (report / (command[1] + '.json')).write_text(result.stdout)
        if result.returncode:
            raise RuntimeError('Jcode command failed; inspect local report: ' + str(report))
        return json.loads(result.stdout)
    promotion = cli(['server', 'promote', args.label, '--json'])
    reload_report = cli(['server', 'reload', '--force', '--json']) if args.reload else None
    require(protected(home) == hashes, 'Protected configuration/rules changed during installation; see local backup')
    after = daemons()
    # A live shared daemon must actually execute the new ELF, not merely point
    # at a new channel symlink. Other separately managed daemons are reported.
    if reload_report and reload_report.get('had_listener'):
        deadline = time.monotonic() + 12
        while not any(p['exe'] == str(installed_binary) for p in after):
            if time.monotonic() >= deadline:
                raise RuntimeError('Shared daemon did not activate expected artifact')
            time.sleep(.2)
            after = daemons()
        for daemon in after:
            if daemon['exe'] == str(installed_binary):
                daemon['sha256'] = digest(Path('/proc') / str(daemon['pid']) / 'exe')
                require(daemon['sha256'] == args.sha256, 'Live daemon checksum mismatch')
    result = {'label': args.label, 'version': version, 'sha256': args.sha256,
              'source_commit': args.source_commit, 'protected_files_unchanged': len(hashes),
              'preserved_claude_mcp_exclusion': disable_claude,
              'installed_at_brasilia': datetime.now(ZoneInfo('America/Sao_Paulo')).isoformat(),
              'backup': str(report), 'daemons': after,
              'promotion': promotion, 'reload': reload_report}
    (dest / 'fork-build.json').write_text(json.dumps(result, indent=2) + '\n')
    (report / 'installed.json').write_text(json.dumps(result, indent=2) + '\n')
    print(json.dumps(result, indent=2))


if __name__ == '__main__':
    main()
