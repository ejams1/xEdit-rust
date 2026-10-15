{ parity probe: x-204 }
unit P_x_204;

function Initialize: integer;
var
  i: Integer;
  d: Double;
  s: string;
  b: Boolean;
  sl: TStringList;
  v: Variant;
begin
  sl := TStringList.Create;
  sl.Add('a');
  v := 'x' + nil;
  AddMessage('after');
end;
end.
