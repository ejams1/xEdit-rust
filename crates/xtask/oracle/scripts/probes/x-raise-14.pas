{ parity probe: x-raise-14 }
unit P_x_raise_14;

function Initialize: integer;
begin
  try
    raise Exception.Create('inner');
  except
    on E: Exception do raise Exception.Create('outer: ' + E.Message);
  end;
  AddMessage('after');
end;
end.
