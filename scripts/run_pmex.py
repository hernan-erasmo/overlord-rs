import argparse
import glob
import os
import sys

from datetime import datetime

from dateutil.relativedelta import relativedelta
from dune_client.types import QueryParameter
from dune_client.client import DuneClient
from dune_client.query import QueryBase

import requests

DUNE_API_KEY="klzhPhytFhAn5aDzINUSOOnISUleJdC9" # pmex_api_key, already revoked, use your own
DUNE_API_BASE_URL="https://api.dune.com"
DUNE_API_REQUEST_TIMEOUT=30
PERFORMANCE="medium" # consumes 20 credits out of 2500/month. Lowest performance possible

dune_client = DuneClient(
    api_key=DUNE_API_KEY,
    base_url=DUNE_API_BASE_URL,
    request_timeout=DUNE_API_REQUEST_TIMEOUT,
    performance=PERFORMANCE
)

BEGINNING_DATE = "2023_04"


def run_pmex(data_dir: str) -> None:
    run_again = True
    remaining_query_counter = 2 # failsafe in case query loop goes haywire
    while run_again and remaining_query_counter > 0:
        print(f"run_pmex.py # Beginning query loop. Counter = {remaining_query_counter}", flush=True)
        date_from, date_to, data_file_name, run_again = get_values_for_pmex(data_dir)
        pmex_query = QueryBase(
            name="pmex_automated",
            query_id=4630494,
            params=[
                # yes, the two types of quotes are necessary otherwise the query will fail
                QueryParameter.text_type(name="date_from", value=f"'{date_from}'"),
                QueryParameter.text_type(name="date_to", value=f"'{date_to}'"),
            ],
        )
        execution_results = dune_client.run_query(
            query = pmex_query,
        )
        result_rows = execution_results.result.rows
        assert len(result_rows) == 1, f"Expected 1 row, got {len(result_rows)}. Keys: {[row.keys() for row in result_rows]}"
        result_row = result_rows[0]
        encoded_addresses = result_row['addresses_column'].split(',')
        print(f"Addresses encoded in the response for {data_file_name}: {len(encoded_addresses)}")
        full_data_file_path = f"{data_dir}/vega/borrowers/{data_file_name}"
        with open(full_data_file_path, 'w') as f:
            for address in encoded_addresses:
                f.write(f"{address}\n")
        remaining_query_counter -= 1
        print(f"{len(encoded_addresses)} addresses written to {full_data_file_path}")


def get_values_for_pmex(data_dir: str) -> (str, str, str, bool):
    """
    Depending on the data files available, this function will return the date range
    to be used on the query to Dune, and the file name to be used to store the data.
    """
    borrowers = f"{data_dir}/vega/borrowers"
    if not os.path.exists(borrowers):
        raise ValueError(f"Can't find {borrowers} directory")
    raw_files = glob.glob(f"{borrowers}/*_raw.txt")
    latest_date = max((
        os.path.basename(f).split('_raw.txt')[0]
        for f in raw_files
        if '_raw.txt' in f),
        key=lambda x: datetime.strptime(x, '%Y_%m')
    )
    current_date = datetime.now().strftime("%Y_%m")

    latest_dt = datetime.strptime(latest_date, '%Y_%m')
    current_dt = datetime.strptime(current_date, '%Y_%m')
    months_diff = (current_dt.year - latest_dt.year) * 12 + (current_dt.month - latest_dt.month)

    if months_diff == 1:
        return (
            current_dt.strftime("%Y-%m-01 00:00"),
            (current_dt + relativedelta(months=1, days=-1)).replace(hour=23, minute=59).strftime("%Y-%m-%d %H:%M"),
            f"{current_dt.strftime('%Y_%m')}_raw_partial.txt",
            False, # We don't need to run the query again, we already have last month's full data
        )
    elif months_diff == 2:
        previous_month_dt = current_dt + relativedelta(months=-1)
        return (
            previous_month_dt.replace(hour=0, minute=0).strftime("%Y-%m-%d %H:%M"),
            (previous_month_dt + relativedelta(months=1, days=-1)).replace(hour=23, minute=59).strftime("%Y-%m-%d %H:%M"),
            f"{previous_month_dt.strftime('%Y_%m')}_raw.txt",
            True, # We need to run the query again, first to finalize the previous month, and then to get the partial
        )
    elif months_diff > 2:
        raise ValueError("Data is more than 2 months ahead of the latest data. Rectify this manually.")
    elif months_diff <= 0:
        raise ValueError("This should've never happened. Investigate this.")


def generate_addresses_file(data_dir: str) -> str:
    print("run_pmex.py # Generating addresses file", flush=True)
    output_dir = f"{data_dir}/vega"
    borrowers = f"{data_dir}/vega/borrowers"

    # Find all raw files and collect addresses
    addresses = set()
    for filepath in glob.glob(f"{borrowers}/*_raw.txt"):
        with open(filepath, 'r') as f:
            addresses.update(line.strip() for line in f if line.strip())

    # Find all raw_partial files and collect addresses
    for filepath in glob.glob(f"{borrowers}/*_raw_partial.txt"):
        with open(filepath, 'r') as f:
            addresses.update(line.strip() for line in f if line.strip())

    # Generate output filename with timestamp and count
    timestamp = datetime.now().strftime("%Y%m%d%H%M%S")
    address_count = len(addresses)
    output_file = f"{output_dir}/addresses_{timestamp}_{address_count}.txt"

    # Write unique addresses to output file
    with open(output_file, 'w') as f:
        for address in sorted(addresses):
            f.write(f"{address}\n")

    print(f"run_pmex.py # Generated file with {address_count} addresses", flush=True)
    return output_file

def main():
    data_dir = os.getenv('DATA_DIR')
    if not data_dir:
        print("run_pmex.py # DATA_DIR environment variable not set or empty", 
              file=sys.stderr, flush=True)
        return 1
    print(f"run_pmex.py # Running with DATA_DIR = {data_dir}", flush=True)

    parser = argparse.ArgumentParser(description='Process PMEX data')
    parser.add_argument('--force-pmex-update', type=str, default="false",
                       help='If true, force running PMEX query on Dune (CONSUMES CREDITS)')
    
    try:
        args = parser.parse_args()
        if args.force_pmex_update.lower() == "true":
            run_pmex(data_dir)
        else:
            print("run_pmex.py # USING STALE DATA. Skipping PMEX query execution.", flush=True)
        result = generate_addresses_file(data_dir)
        print(result, flush=True)
        return 0
    except Exception as e:
        print(f"run_pmex.py # Error: {str(e)}", file=sys.stderr, flush=True)
        return 1


if __name__ == "__main__":
    sys.exit(main())
