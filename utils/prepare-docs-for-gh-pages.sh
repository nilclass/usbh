#!/bin/bash

# This script is called from the workflow in .github/workflows/docs.yml

set -e
set -x

# verify that gh-pages directory does not exist
[ ! -e gh-pages ]

# copy doc directory
cp -a target/doc gh-pages

# create index.html to redirect to the proper location
# (source: https://dev.to/deciduously/prepare-your-rust-api-docs-for-github-pages-2n5i)
echo '<meta http-equiv="refresh" content="0; url=usbh">' > gh-pages/index.html

