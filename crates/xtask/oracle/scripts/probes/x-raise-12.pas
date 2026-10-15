{ parity probe: x-raise-12 }
unit P_x_raise_12;

function Initialize: integer;
begin
  try
    raise Exception.Create('inner');
  except
    AddMessage('caught');
  end;
  AddMessage('after');
end;
end.
