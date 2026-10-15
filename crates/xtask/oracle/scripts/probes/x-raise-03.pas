{ parity probe: x-raise-03 }
unit P_x_raise_03;

function Initialize: integer;
begin
  try
    raise Exception.Create('inner');
  except
    on E: Exception do AddMessage(E.ClassName + ': ' + E.Message);
  end;
  AddMessage('after');
end;
end.
