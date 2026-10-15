{ parity probe: x-raise-04 }
unit P_x_raise_04;

function Initialize: integer;
var
  i: Integer;
begin
  try
    i := 'x';
  except
    on E: Exception do AddMessage(E.ClassName + ': ' + E.Message);
  end;
  AddMessage('after');
end;
end.
