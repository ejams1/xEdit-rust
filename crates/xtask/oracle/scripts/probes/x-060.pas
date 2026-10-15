{ parity probe: x-060 }
unit P_x_060;

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
  sl.Nope(1);
  AddMessage('after');
end;
end.
