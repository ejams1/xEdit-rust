{ parity probe: x-raise-05 }
unit P_x_raise_05;

function Initialize: integer;
var
  i: Integer;
begin
  try
    i := 7 div 0;
  except
    on E: Exception do AddMessage(E.ClassName + ': ' + E.Message);
  end;
  AddMessage('after');
end;
end.
