{ parity probe: x-105 }
unit P_x_105;

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
  AddMessage(#0 + 'zero');
  AddMessage('after');
end;
end.
