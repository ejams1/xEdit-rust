{ parity probe: x-raise-10 }
unit P_x_raise_10;

function Initialize: integer;
begin
  try
    Foo(1);
  except
    on E: Exception do AddMessage(E.ClassName + ': ' + E.Message);
  end;
  AddMessage('after');
end;
end.
