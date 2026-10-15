{ parity probe: x-raise-16 }
unit P_x_raise_16;

function Initialize: integer;
begin
  try
    raise Exception.Create('inner');
  finally
    AddMessage('finally');
  end;
  AddMessage('after');
end;
end.
