{ parity probe: x-308 }
unit P_x_308;

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
  sl.Exchange(0, 5);
  AddMessage('after');
end;
end.
