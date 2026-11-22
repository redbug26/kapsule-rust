#!/usr/bin/env python3
"""Check that public artifacts cannot smuggle server-executable files or symlinks."""
import base64
import json
import os
from pathlib import Path
import shutil
import tempfile
from web_distribution import verify, seal

ROOT=Path(__file__).resolve().parents[1]
with tempfile.TemporaryDirectory(prefix='kapsule-security-test-') as temporary:
    web=Path(temporary)/'web'
    shutil.copytree(Path(os.environ.get('KAPSULE_WEB_DIR', ROOT/'dist/web')),web)
    verify(web)
    def rejected():
        try: verify(web)
        except (ValueError,OSError): return
        raise AssertionError('Unsafe distribution accepted')
    secret=web/'.env';secret.write_text('test');rejected();secret.unlink()
    link=web/'unexpected-link';link.symlink_to(web/'index.html');rejected();link.unlink()
    catalog=json.loads((web/'store/catalog.json').read_text())
    app=catalog['applications'][0]
    package_path=web/'store'/catalog['packages'][app['id']]
    payload=json.loads(package_path.read_text())
    prefix=app['descriptor'][:-len('apps.toml')]
    for name in ['server.php','.htaccess']:
        data=b'<?php this must remain inert data ?>'
        payload['files'].append({'path':name,'data':base64.b64encode(data).decode()})
        catalog['files'].append({'path':prefix+name,'size':len(data)})
    package_path.write_text(json.dumps(payload))
    (web/'store/catalog.json').write_text(json.dumps(catalog))
    seal(web);verify(web)
    assert not list((web/'store').rglob('*.php'))
    assert not list((web/'store').rglob('.htaccess'))
    (web/'app.js').write_text('changed');rejected()
print('Security: unexpected files, symlinks and tampering rejected; PHP and Apache configuration remain inert package data')
