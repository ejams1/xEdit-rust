{ parity probe: x-raise-13 }
unit P_x_raise_13;

function Initialize: integer;
begin
  try
    raise Exception.Create('inner');
  except
    on E: Exception do raise;
  end;
  AddMessage('after');
end;
end.
