{ parity probe: x-raise-11 }
unit P_x_raise_11;

function Initialize: integer;
begin
  try
    raise Exception.Create('inner');
  except
    on E: EConvertError do AddMessage('conv');
  end;
  AddMessage('after');
end;
end.
