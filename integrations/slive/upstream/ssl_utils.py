import os
import ssl

import certifi


def create_download_ssl_context():
    """Trust public CAs bundled with the app plus local Windows root CAs."""
    context = ssl.create_default_context(cafile=certifi.where())
    if os.name == "nt":
        context.load_default_certs(ssl.Purpose.SERVER_AUTH)
    return context
