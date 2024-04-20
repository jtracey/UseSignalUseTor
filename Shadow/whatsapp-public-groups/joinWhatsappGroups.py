# script to go through a list of whatsapp groups and join them.

from selenium import webdriver
from selenium.webdriver.remote.remote_connection import LOGGER
from selenium.webdriver.support.ui import WebDriverWait

import os
import sys
import time
import random
import logging
LOGGER.setLevel(logging.WARNING)


directory = "group_data_html/"
if not os.path.exists(directory):
    os.makedirs(directory)

# Replace below path with the absolute path
# to chromedriver in your computer

driver = webdriver.Chrome()
driver.set_page_load_timeout(15)

filename = sys.argv[1]
f = open(filename)  # file containing the links to the whatsapp groups
lines = f.readlines()
count = 1

driver.get("https://web.whatsapp.com")
wait = WebDriverWait(driver, 600)

print("waiting...")
userwait = input("Scan the QR code, then press enter here.")
print("done waiting")

for line in lines:
    line = line.strip().strip("/")
    group_id = line.split("/")[-1]
    url = 'https://web.whatsapp.com/accept?code={}'.format(group_id)
    print("processing", line)
    driver.get(url)
    sleep_time = 12
    for i in range(1, 2):
        #join_button = driver.find_element_by_css_selector("#action-button")
        #join_button.click()
        #print("clicked join button", group_id)
        #sleep_time = random.randint(20, 30)
        print("sleeping for", sleep_time)
        time.sleep(sleep_time)  # allow time for page to load
        #input("Wait for the page to load, then press enter here.")
        #webLink = driver.find_element_by_link_text("use WhatsApp Web")
        #webLink.click()
        #print("clicked 'use WhatsApp web'")
        #time.sleep(sleep_time)
        try:
            join_group_button = driver.find_element_by_xpath(
                '//div[@data-testid="popup-controls-ok"]')
        except:
            print("failed to join group, trying again...")
            time.sleep(sleep_time)
            try:
                join_group_button = driver.find_element_by_xpath(
                    '//div[@data-testid="popup-controls-ok"]')
            except:
                print("still failing")
                try:
                    failure = driver.find_element_by_xpath(
                        '//div[@data-testid="popup-contents"]')
                    print("failure reason: {}", failure.get_attribute('value'))
                except:
                    print("could not find reason, continuing")
                continue
        #out = open(directory + "/" + group_id, "w")
        #print("saving info")
        #out.write(group_info.get_attribute('innerHTML') + "\n")
        #out.close()
        #join_group_button = driver.find_element_by_css_selector(
        #    ".btn-plain.btn-default.popup-controls-item")
        print("joining group", group_id)
        join_group_button.click()

driver.close()
